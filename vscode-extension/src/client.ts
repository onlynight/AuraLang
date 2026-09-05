/**
 * Aura LSP 客户端
 *
 * 负责启动 `aura lsp` 进程，通过 JSON-RPC (stdio) 通信，
 * 将 LSP 功能暴露给 VS Code。
 */

import * as vscode from "vscode";
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
} from "vscode-languageclient/node";
import { DiagnosticManager } from "./diagnostics";

// 单例客户端
let client: LanguageClient | undefined;
let diagManager: DiagnosticManager | undefined;

/**
 * 启动 LSP 客户端
 */
export async function startLSPClient(
    context: vscode.ExtensionContext,
    dm: DiagnosticManager
): Promise<void> {
    diagManager = dm;
    const config = vscode.workspace.getConfiguration("aura");

    const serverPath = config.get<string>("serverPath") || "aura";
    const serverArgs = config.get<string[]>("serverArgs") || [];
    const serverEnv = config.get<Record<string, string>>("serverEnv") || {};

    // 如果 serverPath 已包含 "lsp"，不重复添加
    const fullArgs = serverPath.includes("lsp")
        ? serverArgs
        : ["lsp", ...serverArgs];

    const cwd =
        vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || ".";

    const serverOptions: ServerOptions = {
        run: {
            module: undefined,
            transport: TransportKind.stdio,
            command: serverPath,
            args: fullArgs,
            options: {
                env: { ...process.env, ...serverEnv },
                cwd,
            },
        },
        debug: {
            transport: TransportKind.stdio,
            command: serverPath,
            args: fullArgs,
            options: {
                env: { ...process.env, ...serverEnv, DEBUG: "true" },
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

    // 启动客户端
    await client.start();

    client.onDidChangeState((e) => {
        const states = ["未启动", "已启动", "已停止", "已错误"];
        const prev = states[e.oldState];
        const curr = states[e.newState];
        console.log(`[Aura] LSP 状态变更: ${prev} -> ${curr}`);
        if (e.newState === 1) {
            vscode.window.showInformationMessage("Aura LSP 服务器已连接");
        } else if (e.newState === 2) {
            console.log("[Aura] LSP 服务器已停止");
        }
    });

    console.log("[Aura] LSP 客户端已启动");
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
 * 发送诊断请求
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
                edit.insert(uri, new vscode.Position(0, 0), te.newText);
            }
            await vscode.workspace.applyEdit(edit);
            vscode.window.showInformationMessage("Aura 文档已格式化");
        }
    } catch (err) {
        console.error("[Aura] 格式化失败:", err);
        vscode.window.showErrorMessage(`Aura 格式化失败: ${err}`);
    }
}