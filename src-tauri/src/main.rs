#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args().skip(1);
    if matches!(args.next().as_deref(), Some("acp")) {
        match args.next().as_deref() {
            Some("--stdio") => {
                if let Err(error) = tiycode_lib::run_acp_stdio() {
                    eprintln!("failed to run TiyCode ACP stdio server: {error}");
                    std::process::exit(1);
                }
                return;
            }
            Some("--http") => {
                let addr = args.next().unwrap_or_else(|| {
                    eprintln!("usage: tiycode acp --http <addr>");
                    std::process::exit(1);
                });
                if let Err(error) = tiycode_lib::run_acp_http(&addr) {
                    eprintln!("failed to run TiyCode ACP HTTP server: {error}");
                    std::process::exit(1);
                }
                return;
            }
            other => {
                eprintln!(
                    "unknown acp transport '{}'. usage: tiycode acp --stdio | tiycode acp --http <addr>",
                    other.unwrap_or("")
                );
                std::process::exit(1);
            }
        }
    }

    tiycode_lib::run()
}
