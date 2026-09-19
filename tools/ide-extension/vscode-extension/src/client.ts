/**
 * Aura LSP 客户端
 *
 * 负责启动扩展**自带**的 `aura-lsp` 二进制（位于 `extensionPath/bin/`），
 * 通过 JSON-RPC (stdio) 通信，将 LSP 功能暴露给 VS Code。
 *
 * 扩展直接启动独立的 `aura-lsp` 二进制（不再经由 `aura.exe lsp` 中转，
 * 也不再依赖系统 PATH 或工作区构建产物），因此：
 * - 用户从 Marketplace 安装后开箱即用，无需手动编译 Aura。
 * - `target/debug/aura.exe` 不会被 LSP 进程占用，`cargo build` 可正常覆盖。
 *
 * 解析顺序：用户显式路径 → 内置 bin/aura-lsp → 工作区构建产物 → PATH。
 *
 * @version 0.1.9
 */

import * as vscode from "vscode";
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    State,
    TransportKind,
} from "vscode-languageclient/node";
import { DiagnosticManager } from "./diagnostics";

// 单例客户端
let client: LanguageClient | undefined;
let diagManager: DiagnosticManager | undefined;

/**
 * 查找可用的 aura-lsp 可执行文件。
 *
 * 扩展**自带**独立的 `aura-lsp` 二进制（打包在 `extensionPath/bin/` 下），
 * 不再依赖系统 PATH 或工作区构建产物，也不再经由 `aura.exe lsp` 中转。
 * 这样 `target/debug/aura.exe` 不会被 LSP 进程占用，`cargo build`
 * debug profile 可正常覆盖它；用户从 Marketplace 安装后开箱即用。
 *
 * 查找顺序（高优先级在前）：
 * 1. 用户配置的 aura.serverPath（仅当显式设置且不为默认值 "aura-lsp"，
 *    且指向存在的绝对路径 / 相对路径时）——用于本地开发自编译版本。
 * 2. 扩展内置的 `bin/aura-lsp[.exe]`（默认，推荐）。
 * 3. 打开的工作区内的构建产物：`target/debug`、`target/release`、`bin`
 *    ——开发 Aura 本身时使用。
 * 4. 回退到 PATH 中的 `aura-lsp` 命令。
 */
async function findServerCommand(
    config: vscode.WorkspaceConfiguration,
    extensionPath: string
): Promise<{ command: string; args: string[] }> {
    const fs = await import("fs");
    const path = await import("path");

    const serverArgs = config.get<string[]>("serverArgs") || [];
    // aura-lsp 是独立二进制，不需要 "lsp" 子命令中转
    const fullArgs = serverArgs;

    const isWin = process.platform === "win32";
    const exe = isWin ? "aura-lsp.exe" : "aura-lsp";

    const rawConfig = config.get<string | undefined>("serverPath");
    // 只有当用户显式覆盖了默认值时才把它当作"用户意图"
    const userOverrode =
        rawConfig !== undefined && rawConfig !== "" && rawConfig !== "aura-lsp";

    // 1. 用户显式覆盖：绝对路径 / 显式带分隔符的路径 —— 存在则直接用
    if (userOverrode && rawConfig) {
        const looksLikePath =
            path.isAbsolute(rawConfig) ||
            rawConfig.includes("/") ||
            rawConfig.includes("\\");
        if (looksLikePath) {
            try {
                if (fs.existsSync(rawConfig)) {
                    console.log(
                        `[Aura] Using user-configured aura-lsp path: ${rawConfig}`
                    );
                    return { command: rawConfig, args: fullArgs };
                }
            } catch {
                /* ignore */
            }
            // 路径不存在 —— 提示后继续走内置兜底
            console.warn(
                `[Aura] Configured aura.serverPath does not exist, falling back to bundled binary: ${rawConfig}`
            );
        } else {
            // 简单命令名（如 "aura"、"aura-lsp"）：不是内置二进制的话，
            // 直接跳过，避免误启动 aura.exe 或其它同名程序。
            if (rawConfig === exe || rawConfig === "aura-lsp" || rawConfig === "aura") {
                console.log(
                    `[Aura] aura.serverPath="${rawConfig}" is treated as default value, using bundled binary`
                );
            } else {
                // 用户指定了别的命令名，交给 PATH 处理
                console.log(`[Aura] Using user-specified command: ${rawConfig}`);
                return { command: rawConfig, args: fullArgs };
            }
        }
    }

    // 2. 扩展内置二进制（默认路径）
    const bundled = path.join(extensionPath, "bin", exe);
    try {
        if (fs.existsSync(bundled)) {
            // Unix 下确保可执行位（VSIX 打包会保留，但源码构建时保险）
            if (!isWin) {
                try {
                    fs.chmodSync(bundled, 0o755);
                } catch {
                    /* ignore */
                }
            }
            console.log(`[Aura] Using bundled aura-lsp: ${bundled}`);
            return { command: bundled, args: fullArgs };
        }
    } catch (err) {
        console.warn(`[Aura] Failed to check bundled aura-lsp: ${err}`);
    }

    // 3. 在工作区查找构建产物（Aura 项目自身开发时使用）
    for (const folder of vscode.workspace.workspaceFolders || []) {
        const root = folder.uri.fsPath;
        const candidates = [
            path.join(root, "target", "debug", exe),
            path.join(root, "target", "release", exe),
            path.join(root, "bin", exe),
        ];
        for (const c of candidates) {
            try {
                if (fs.existsSync(c)) {
                    console.log(`[Aura] Using workspace aura-lsp: ${c}`);
                    return { command: c, args: fullArgs };
                }
            } catch {
                /* ignore */
            }
        }
    }

    // 4. 回退：让 VS Code 通过 PATH 查找
    console.warn(`[Aura] Bundled aura-lsp missing, trying ${exe} in PATH`);
    return { command: exe, args: fullArgs };
}

/**
 * 启动 LSP 客户端
 */
export async function startLSPClient(
    context: vscode.ExtensionContext,
    dm: DiagnosticManager
): Promise<void> {
    diagManager = dm;
    const config = vscode.workspace.getConfiguration("aura");

    let server: { command: string; args: string[] };
    try {
        server = await findServerCommand(config, context.extensionPath);
    } catch (err) {
        console.error("[Aura] Failed to find aura executable:", err);
        vscode.window.showErrorMessage(
            "Aura: aura-lsp executable not found. Extension should bundle bin/aura-lsp; if missing, please check installation integrity or set aura.serverPath to a local build artifact."
        );
        return;
    }

    const cwd =
        vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || ".";

    const serverOptions: ServerOptions = {
        run: {
            transport: TransportKind.stdio,
            command: server.command,
            args: server.args,
            options: {
                env: { ...process.env, ...config.get("serverEnv") },
                cwd,
            },
        },
        debug: {
            transport: TransportKind.stdio,
            command: server.command,
            args: server.args,
            options: {
                env: { ...process.env, ...config.get("serverEnv"), DEBUG: "true" },
                cwd,
            },
        },
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: "file", language: "aura" }],
        synchronize: {
            fileEvents: [
                vscode.workspace.createFileSystemWatcher("**/*.aura"),
                vscode.workspace.createFileSystemWatcher("**/aura.toml"),
                vscode.workspace.createFileSystemWatcher("**/aura.lock"),
            ],
        },
        outputChannelName: "Aura LSP",
        revealOutputChannelOn: 2,
    };

    client = new LanguageClient(
        "auraLanguageClient",
        "Aura Language Server",
        serverOptions,
        clientOptions
    );

    client.onDidChangeState((e) => {
        // vscode-languageclient v9 的 State 枚举：Stopped=1, Running=2, Starting=3
        console.log(
            `[Aura] LSP state change: state=${e.newState} (${e.newState === State.Starting ? "Starting" : e.newState === State.Running ? "Running" : "Stopped"})`
        );
        if (e.newState === State.Running) {
            vscode.window.showInformationMessage(
                `Aura LSP connected (${server.command})`
            );
        } else if (e.newState === State.Stopped) {
            console.log("[Aura] LSP server stopped");
        }
    });

    try {
        await client.start();
    } catch (err) {
        console.error("[Aura] LSP client startup failed:", err);
        const hint =
            (err as Error)?.message?.includes("ENOENT") ||
            (err as Error)?.message?.includes("spawn")
                ? `(command not found: ${server.command})`
                : "";
        vscode.window.showErrorMessage(
            `Aura LSP failed to start: ${err} ${hint}`.trim() +
                " Extension should bundle bin/aura-lsp; if the bundled binary is missing, please reinstall the extension or set aura.serverPath to a local build artifact and retry."
        );
        client = undefined;
    }

    console.log("[Aura] LSP client startup complete");
}

/**
 * 停止 LSP 客户端
 */
export async function stopLSPClient(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
        console.log("[Aura] LSP client stopped");
    }
}

/**
 * 获取客户端实例
 */
export function getLSPClient(): LanguageClient | undefined {
    return client;
}

/**
 * 发送诊断请求（手动命令）
 */
export async function sendDiagnosticRequest(
    uri: vscode.Uri
): Promise<void> {
    if (!client || !diagManager) return;

    try {
        const result = await client.sendRequest(
            "textDocument/diagnostic",
            { textDocument: { uri: uri.toString() } }
        );
        console.log("[Aura] Diagnostic request completed:", result);
        diagManager.updateDiagnostics(uri, result);
    } catch (err) {
        console.error("[Aura] Diagnostic request failed:", err);
    }
}

/**
 * 格式化文档
 */
export async function formatDocument(
    uri: vscode.Uri
): Promise<void> {
    if (!client) return;

    try {
        const result = await client.sendRequest(
            "textDocument/formatting",
            {
                textDocument: { uri: uri.toString() },
                options: { tabSize: 4, insertSpaces: true },
            }
        );

        if (result && Array.isArray(result) && result.length > 0) {
            const edit = new vscode.WorkspaceEdit();
            for (const te of result as any[]) {
                if (te && te.range) {
                    edit.replace(
                        uri,
                        new vscode.Range(
                            te.range.start.line,
                            te.range.start.character,
                            te.range.end.line,
                            te.range.end.character
                        ),
                        te.newText
                    );
                }
            }
            await vscode.workspace.applyEdit(edit);
            vscode.window.showInformationMessage("Aura document formatted");
        }
    } catch (err) {
        console.error("[Aura] Formatting failed:", err);
        vscode.window.showErrorMessage(`Aura formatting failed: ${err}`);
    }
}