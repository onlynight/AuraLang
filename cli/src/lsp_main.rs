//! Aura LSP 独立二进制入口
//!
//! 作为独立进程运行，通过 stdio 与编辑器通信。
//! 从 `aura.exe` 中拆出，避免将 VM/AOT/JIT 等无关代码链接进 LSP 进程。
//!
//! 用法：
//!   aura-lsp                    # 通过 stdio 通信（默认）
//!   aura-lsp --port <port>      # 通过 TCP socket 通信（预留）

fn main() {
    // 解析命令行参数（当前仅支持 stdio 模式）
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Aura LSP Server v0.1.0");
        println!("通过 stdio (JSON-RPC) 与编辑器通信");
        return;
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("aura-lsp 0.1.0");
        return;
    }

    eprintln!("[aura-lsp] LSP 服务器启动 (stdio 模式)");
    compiler::lsp::run_lsp_server();
}
