use std::process::ExitCode;

fn main() -> ExitCode {
    let out = std::env::args().nth(1).unwrap_or_else(|| "frame.png".into());

    match rift_core::grab::primary_monitor_to_png(&out) {
        Ok(()) => {
            println!("saved {out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("capture failed: {e}");
            ExitCode::FAILURE
        }
    }
}
