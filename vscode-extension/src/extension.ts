/**
 * Aura Language — VS Code 扩展
 *
 * 提供：
 * - LSP 语言服务器连接（aura lsp）
 * - 代码补全、跳转定义、悬停提示、诊断推送
 * - 代码格式化
 * - 保存时检查
 *
 * @version 0.1.0
 * @license MIT
 */

import * as vscode from "vscode";
import { startLSPClient, stopLSPClient, sendDiagnosticRequest, formatDocument } from "./client";
import { DiagnosticManager } from "./diagnostics";

// 扩展状态
let diagnosticManager: DiagnosticManager;

/**
 * 激活扩展
 */
export async function activate(context: vscode.ExtensionContext): Promise<void> {
    console.log("[Aura] 扩展激活");

    // 初始化诊断管理器
    diagnosticManager = new DiagnosticManager();
    context.subscriptions.push(diagnosticManager);

    // 启动 LSP 客户端
    await startLSPClient(context, diagnosticManager);

    // 注册命令
    context.subscriptions.push(
        vscode.commands.registerCommand("aura.restartLSP", async () => {
            await vscode.window.showInformationMessage("重启 Aura LSP 服务器...");
            await stopLSPClient();
            await startLSPClient(context, diagnosticManager);
            vscode.window.showInformationMessage("Aura LSP 服务器已重启");
        }),
        vscode.commands.registerCommand("aura.openDiagnostic", async () => {
            const currentEditor = vscode.window.activeTextEditor;
            if (!currentEditor || currentEditor.document.languageId !== "aura") {
                vscode.window.showWarningMessage("请打开一个 Aura 文件");
                return;
            }
            await sendDiagnosticRequest(currentEditor.document.uri);
        }),
        vscode.commands.registerCommand("aura.formatDocument", async () => {
            const currentEditor = vscode.window.activeTextEditor;
            if (!currentEditor || currentEditor.document.languageId !== "aura") {
                vscode.window.showWarningMessage("请打开一个 Aura 文件");
                return;
            }
            await formatDocument(currentEditor.document.uri);
        }),
        vscode.commands.registerCommand("aura.showVersion", async () => {
            vscode.window.showInformationMessage("Aura Language v0.1.5");
        })
    );

    // 监听文档保存
    context.subscriptions.push(
        vscode.workspace.onDidSaveTextDocument(async (doc) => {
            if (doc.languageId !== "aura") return;
            const config = vscode.workspace.getConfiguration("aura");

            if (config.get<boolean>("formatOnSave")) {
                await formatDocument(doc.uri);
            }
        })
    );

    // 监听配置变更
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(async (e) => {
            if (e.affectsConfiguration("aura")) {
                console.log("[Aura] 配置变更，重启 LSP");
                await stopLSPClient();
                await startLSPClient(context, diagnosticManager);
            }
        })
    );

    // 注册文档打开/变更回调 —— 实时诊断由 LSP 客户端（拉取模式）负责，
    // 这里无需手动调度。仅保留显式命令 aura.openDiagnostic。

    console.log("[Aura] 扩展激活完成");
}

/**
 * 停用扩展
 */
export async function deactivate(): Promise<void> {
    console.log("[Aura] 扩展停用");
    await stopLSPClient();
}