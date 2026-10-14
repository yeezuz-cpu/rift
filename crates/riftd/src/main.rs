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
        Some("buffer") => {
            let run_secs = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(15);
            let seg_secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
            let keep = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
            rift_core::buffer::run(rift_core::buffer::Opts {
                dir: std::env::temp_dir().join("rift-buf"),
                seg_secs,
                keep,
                run_secs,
                fps: 60,
            })
        }
        _ => {
            eprintln!("usage: riftd grab [out.png] | record [out.mp4] [secs] [fps] | buffer [run] [seg] [keep]");
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
