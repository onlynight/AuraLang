/**
 * 诊断管理器
 *
 * 管理 Aura 编译器的诊断信息（手动 `aura.openDiagnostic` 命令使用）。
 * 常规的实时诊断由 LSP 客户端（vscode-languageclient 的拉取诊断）负责，
 * 本管理器仅用于主动请求并展示一次诊断结果。
 *
 * @version 0.1.3
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
    private collection = vscode.languages.createDiagnosticCollection("aura");

    constructor() {
        // 文档关闭时清理
        vscode.workspace.onDidCloseTextDocument((doc) => {
            if (doc.languageId === "aura") {
                this.diagnostics.delete(doc.uri.toString());
                this.collection.delete(doc.uri);
            }
        });
    }

    /**
     * 更新诊断（单次请求结果）
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
        this.collection.set(uri, diagnostics);
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
        this.diagnostics.clear();
        this.collection.dispose();
    }
}