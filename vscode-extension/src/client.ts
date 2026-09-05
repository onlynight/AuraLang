/**
 * Aura LSP 客户端
 *
 * 负责启动 `aura lsp` 进程，通过 JSON-RPC (stdio) 通信，
 * 将 LSP 功能暴露给 VS Code。
 *
 * @version 0.1.3
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
 * 查找可用的 aura 可执行文件。
 *
 * 查找顺序：
 * 1. 用户配置的 aura.serverPath（绝对路径或含分隔符的路径，存在则直接用）
 * 2. 打开的工作区内的构建产物：target/debug、target/release、bin 目录
 * 3. 回退到 PATH 中的 `aura` 命令
 */
async function findServerCommand(
    config: vscode.WorkspaceConfiguration
): Promise<{ command: string; args: string[] }> {
    const fs = await import("fs");
    const path = await import("path");

    const serverPath = config.get<string>("serverPath") || "aura";
    const serverArgs = config.get<string[]>("serverArgs") || [];
    // 如果 serverPath 已包含 "lsp"，不重复添加
    const fullArgs = serverPath.includes("lsp")
        ? serverArgs
        : ["lsp", ...serverArgs];

    const isWin = process.platform === "win32";
    const exe = isWin ? "aura.exe" : "aura";

    // 1. 配置为绝对路径 / 显式带路径
    if (
        path.isAbsolute(serverPath) ||
        serverPath.includes("/") ||
        serverPath.includes("\\")
    ) {
        try {
            if (fs.existsSync(serverPath)) {
                return { command: serverPath, args: fullArgs };
            }
        } catch {
            /* ignore */
        }
    }

    // 2. 在工作区查找构建产物
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
                    console.log(
                        `[Aura] 使用工作区内的 aura 二进制: ${c}`
                    );
                    return { command: c, args: fullArgs };
                }
            } catch {
                /* ignore */
            }
        }
    }

    // 3. 回退：让 VS Code 通过 PATH 查找
    return { command: serverPath, args: fullArgs };
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
        server = await findServerCommand(config);
    } catch (err) {
        console.error("[Aura] 查找 aura 可执行文件失败:", err);
        vscode.window.showErrorMessage(
            "Aura: 未找到 aura 可执行文件。请设置 aura.serverPath 或打开包含 target/debug/aura 的工作区。"
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
            `[Aura] LSP 状态变更: state=${e.newState} (${e.newState === State.Starting ? "启动中" : e.newState === State.Running ? "运行中" : "已停止"})`
        );
        if (e.newState === State.Running) {
            vscode.window.showInformationMessage(
                `Aura LSP 已连接 (${server.command})`
            );
        } else if (e.newState === State.Stopped) {
            console.log("[Aura] LSP 服务器已停止");
        }
    });

    try {
        await client.start();
    } catch (err) {
        console.error("[Aura] LSP 客户端启动失败:", err);
        const hint =
            (err as Error)?.message?.includes("ENOENT") ||
            (err as Error)?.message?.includes("spawn")
                ? `（未找到命令 ${server.command}）`
                : "";
        vscode.window.showErrorMessage(
            `Aura LSP 启动失败: ${err} ${hint}`.trim() +
                " 请检查 aura.serverPath 配置，或打开包含 target/debug/aura 的工作区后重启。"
        );
        client = undefined;
    }

    console.log("[Aura] LSP 客户端启动流程完成");
}

/**
 * 停止 LSP 客户端
 */
export async function stopLSPClient(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
        console.log("[Aura] LSP 客户端已停止");
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
        console.log("[Aura] 诊断请求完成:", result);
        diagManager.updateDiagnostics(uri, result);
    } catch (err) {
        console.error("[Aura] 诊断请求失败:", err);
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
            vscode.window.showInformationMessage("Aura 文档已格式化");
        }
    } catch (err) {
        console.error("[Aura] 格式化失败:", err);
        vscode.window.showErrorMessage(`Aura 格式化失败: ${err}`);
    }
}