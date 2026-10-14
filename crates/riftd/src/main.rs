use std::process::ExitCode;

use rift_core::record::Opts;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let res = match args.first().map(String::as_str) {
        Some("grab") => {
            let out = args.get(1).map(String::as_str).unwrap_or("frame.png");
            rift_core::grab::primary_monitor_to_png(out)
        }
        Some("record") => {
            let path = args.get(1).cloned().unwrap_or("out.mp4".into());
            let secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
            let fps = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);
            rift_core::record::record_primary(Opts { path, secs, fps })
        }
        _ => {
            eprintln!("usage: riftd grab [out.png] | record [out.mp4] [secs] [fps]");
            return ExitCode::FAILURE;
        }
    };

    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("failed: {e}");
            ExitCode::FAILURE
        }
    }
}
