//! Aura LSP standalone binary entry point
//!
//! Runs as a separate process and communicates with the editor via stdio.
//! Split out from `aura.exe` to avoid linking unrelated VM/AOT/JIT code into the LSP process.
//!
//! Usage:
//!   auralsp                    # Communicate via stdio (default)
//!   auralsp --port <port>      # Communicate via TCP socket (reserved)

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Aura LSP Server v0.1.0");
        println!("Communicates with the editor via stdio (JSON-RPC)");
        return;
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("auralsp 0.1.0");
        return;
    }

    eprintln!("[auralsp] LSP server starting (stdio mode)");
    compiler::lsp::run_lsp_server();
}
