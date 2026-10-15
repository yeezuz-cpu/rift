mod hotkey;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rift_core::buffer;
use rift_core::record::Opts;

fn arg<T: std::str::FromStr>(args: &[String], i: usize, default: T) -> T {
    args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn clips_dir() -> PathBuf {
    match std::env::var("USERPROFILE") {
        Ok(p) => PathBuf::from(p).join("Videos").join("Rift"),
        Err(_) => PathBuf::from("clips"),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let res = match args.first().map(String::as_str) {
        Some("grab") => {
            let out = args.get(1).map(String::as_str).unwrap_or("frame.png");
            rift_core::grab::primary_monitor_to_png(out)
        }
        Some("record") => {
            let path = args.get(1).cloned().unwrap_or("out.mp4".into());
            rift_core::record::record_primary(Opts {
                path,
                secs: arg(&args, 2, 10),
                fps: arg(&args, 3, 60),
            })
        }
        Some("buffer") => buffer::run(
            buffer::Opts {
                dir: std::env::temp_dir().join("rift-buf"),
                out_dir: clips_dir(),
                seg_secs: arg(&args, 2, 3),
                keep: arg(&args, 3, 4),
                run_secs: arg(&args, 1, 15),
                fps: 60,
            },
            Arc::new(AtomicBool::new(false)),
        ),
        Some("run") => {
            let clip_secs: u64 = arg(&args, 1, 30);
            let seg_secs: u64 = arg(&args, 2, 3);
            let save = Arc::new(AtomicBool::new(false));
            hotkey::spawn(save.clone());
            println!("keeping the last ~{clip_secs}s, F9 saves a clip, Ctrl+C quits");
            buffer::run(
                buffer::Opts {
                    dir: std::env::temp_dir().join("rift-buf"),
                    out_dir: clips_dir(),
                    seg_secs,
                    // +1 because the segment cut by the hotkey is a short one
                    keep: (clip_secs.div_ceil(seg_secs) + 1) as usize,
                    run_secs: 0,
                    fps: 60,
                },
                save,
            )
        }
        Some("clip") => {
            let out = args.get(1).cloned().unwrap_or("clip.mp4".into());
            let dir = std::env::temp_dir().join("rift-buf");
            let mut files: Vec<_> = std::fs::read_dir(&dir)
                .map(|d| d.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
            files.sort();
            rift_core::clip::join(&files, out.as_ref()).map(|lens| {
                let total: f64 = lens.iter().sum();
                println!("{} segments {:.2?} = {:.2}s -> {}", lens.len(), lens, total, out);
            })
        }
        _ => {
            eprintln!(
                "usage: riftd run [clip_secs] [seg_secs]\n       riftd grab [out.png] | record [out.mp4] [secs] [fps]\n       riftd buffer [run] [seg] [keep] | clip [out.mp4]"
            );
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
