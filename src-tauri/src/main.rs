#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args().skip(1);
    if matches!(args.next().as_deref(), Some("acp"))
        && matches!(args.next().as_deref(), Some("--stdio"))
    {
        if let Err(error) = tiycode_lib::run_acp_stdio() {
            eprintln!("failed to run TiyCode ACP stdio server: {error}");
            std::process::exit(1);
        }
        return;
    }

    tiycode_lib::run()
}
