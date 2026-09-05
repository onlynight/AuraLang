/**
 * 诊断管理器
 *
 * 管理 Aura 编译器的诊断信息，
 * 包括错误、警告、信息和建议。
 */

import * as vscode from "vscode";

interface LspDiagnostic {
    range: {
        start: { line: number; character: number };
        end: { line: number; character: number };
    };
    severity: number; // 1=Error, 2=Warning, 3=Info, 4=Hint
    code?: string;
    source: string;
    message: string;
}

/**
 * 诊断管理器
 */
export class DiagnosticManager implements vscode.Disposable {
    private diagnostics: Map<string, LspDiagnostic[]> = new Map();
    private scheduledUpdates: Map<string, NodeJS.Timeout> = new Map();

    constructor() {
        // 监听文档打开，自动请求诊断
        vscode.workspace.onDidOpenTextDocument((doc) => {
            if (doc.languageId === "aura") {
                this.scheduleUpdate(doc.uri);
            }
        });

        // 监听文档关闭
        vscode.workspace.onDidCloseTextDocument((doc) => {
            if (doc.languageId === "aura") {
                this.diagnostics.delete(doc.uri.toString());
            }
        });
    }

    /**
     * 调度诊断更新（防抖）
     */
    scheduleUpdate(uri: vscode.Uri, delay: number = 300): void {
        const key = uri.toString();

        // 清除已调度的更新
        const existing = this.scheduledUpdates.get(key);
        if (existing) {
            clearTimeout(existing);
        }

        // 调度新更新
        this.scheduledUpdates.set(
            key,
            setTimeout(async () => {
                this.scheduledUpdates.delete(key);
                await this.requestDiagnostics(uri);
            }, delay)
        );
    }

    /**
     * 请求诊断
     */
    private async requestDiagnostics(uri: vscode.Uri): Promise<void> {
        const client = await import("./client").then((m) => m.getLSPClient());
        if (!client) return;

        try {
            const result = await client.sendRequest(
                "textDocument/diagnostic",
                { textDocument: { uri: uri.toString() } }
            );

            this.updateDiagnostics(uri, result);
        } catch (err) {
            console.error("[Aura] 诊断请求失败:", err);
        }
    }

    /**
     * 更新诊断
     */
    updateDiagnostics(uri: vscode.Uri, raw: unknown): void {
        if (!Array.isArray(raw)) return;

        const key = uri.toString();
        const diagnostics = (raw as LspDiagnostic[]).map((d) => ({
            range: new vscode.Range(
                d.range.start.line,
                d.range.start.character,
                d.range.end.line,
                d.range.end.character
            ),
            severity: this.mapSeverity(d.severity),
            code: d.code,
            source: d.source || "aura",
            message: d.message,
        }));

        this.diagnostics.set(key, raw as LspDiagnostic[]);

        // 发布到 VS Code
        const collection =
            vscode.languages.createDiagnosticCollection("aura");
        collection.set(uri, diagnostics);
    }

    /**
     * 映射严重性
     */
    private mapSeverity(severity: number): vscode.DiagnosticSeverity {
        switch (severity) {
            case 1:
                return vscode.DiagnosticSeverity.Error;
            case 2:
                return vscode.DiagnosticSeverity.Warning;
            case 3:
                return vscode.DiagnosticSeverity.Information;
            case 4:
                return vscode.DiagnosticSeverity.Hint;
            default:
                return vscode.DiagnosticSeverity.Error;
        }
    }

    /**
     * 获取诊断
     */
    getDiagnostics(uri: vscode.Uri): LspDiagnostic[] | undefined {
        return this.diagnostics.get(uri.toString());
    }

    /**
     * 清理
     */
    dispose(): void {
        for (const timeout of this.scheduledUpdates.values()) {
            clearTimeout(timeout);
        }
        this.scheduledUpdates.clear();
        this.diagnostics.clear();
    }
}