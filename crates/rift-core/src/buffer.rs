use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
    VideoSettingsSubType,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::clip;

type Error = Box<dyn std::error::Error + Send + Sync>;

// encoder, its file, and whether a clip should be cut once it's finished
type Segment = (VideoEncoder, PathBuf, bool);

pub struct Opts {
    pub dir: PathBuf,
    pub out_dir: PathBuf,
    pub seg_secs: u64,
    pub keep: usize,
    // 0 = run until the process is killed
    pub run_secs: u64,
    pub fps: u32,
}

fn make_encoder(size: (u32, u32), fps: u32, path: &Path) -> Result<VideoEncoder, Error> {
    let enc = VideoEncoder::new(
        VideoSettingsBuilder::new(size.0, size.1)
            .sub_type(VideoSettingsSubType::H264)
            .frame_rate(fps),
        AudioSettingsBuilder::default().disabled(true),
        ContainerSettingsBuilder::default(),
        path,
    )?;
    Ok(enc)
}

struct Rolling {
    opts: Opts,
    size: (u32, u32),
    save: Arc<AtomicBool>,
    enc: Option<VideoEncoder>,
    cur: PathBuf,
    next: Option<Receiver<(VideoEncoder, PathBuf)>>,
    to_fin: Option<Sender<Segment>>,
    fin_thread: Option<JoinHandle<()>>,
    done: Arc<Mutex<VecDeque<PathBuf>>>,
    idx: u32,
    seg_start: Instant,
    started: Instant,
    swaps: u32,
    worst_call: Duration,
}

impl Rolling {
    fn seg_path(&mut self) -> PathBuf {
        let p = self.opts.dir.join(format!("seg_{:03}.mp4", self.idx));
        self.idx += 1;
        p
    }

    // open the next encoder in the background so the swap is instant
    fn prepare_next(&mut self) {
        let path = self.seg_path();
        let (size, fps) = (self.size, self.opts.fps);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            if let Ok(enc) = make_encoder(size, fps, &path) {
                let _ = tx.send((enc, path));
            }
        });
        self.next = Some(rx);
    }

    fn swap(&mut self, enc: VideoEncoder, path: PathBuf, cut: bool) {
        let old = self.enc.replace(enc).unwrap();
        let old_path = std::mem::replace(&mut self.cur, path);
        let _ = self.to_fin.as_ref().unwrap().send((old, old_path, cut));
        self.next = None;
        self.seg_start = Instant::now();
        self.swaps += 1;
    }
}

fn cut_clip(files: Vec<PathBuf>, out_dir: &Path) {
    let name = chrono::Local::now().format("rift_%Y-%m-%d_%H-%M-%S.mp4").to_string();
    let out = out_dir.join(name);
    let _ = fs::create_dir_all(out_dir);
    match clip::join(&files, &out) {
        Ok(lens) => println!("saved {} ({:.1}s)", out.display(), lens.iter().sum::<f64>()),
        Err(e) => eprintln!("clip failed: {e}"),
    }
}

impl GraphicsCaptureApiHandler for Rolling {
    type Flags = ((u32, u32), Opts, Arc<AtomicBool>);
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (size, opts, save) = ctx.flags;
        let _ = fs::remove_dir_all(&opts.dir);
        fs::create_dir_all(&opts.dir)?;

        let done = Arc::new(Mutex::new(VecDeque::new()));
        let (tx, rx) = mpsc::channel::<Segment>();

        let keep = opts.keep;
        let out_dir = opts.out_dir.clone();
        let done2 = done.clone();
        let fin_thread = thread::spawn(move || {
            for (enc, path, cut) in rx {
                let _ = enc.finish();
                let mut d = done2.lock().unwrap();
                d.push_back(path);
                while d.len() > keep {
                    let old = d.pop_front().unwrap();
                    let _ = fs::remove_file(old);
                }
                if cut {
                    cut_clip(d.iter().cloned().collect(), &out_dir);
                }
            }
        });

        let first = opts.dir.join("seg_000.mp4");
        let enc = make_encoder(size, opts.fps, &first)?;

        Ok(Self {
            opts,
            size,
            save,
            enc: Some(enc),
            cur: first,
            next: None,
            to_fin: Some(tx),
            fin_thread: Some(fin_thread),
            done,
            idx: 1,
            seg_start: Instant::now(),
            started: Instant::now(),
            swaps: 0,
            worst_call: Duration::ZERO,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let t = Instant::now();

        let run = self.opts.run_secs;
        if run > 0 && self.started.elapsed() >= Duration::from_secs(run) {
            let last = self.enc.take().unwrap();
            let _ = self.to_fin.as_ref().unwrap().send((last, self.cur.clone(), false));
            self.to_fin = None;
            self.fin_thread.take().unwrap().join().ok();

            let done = self.done.lock().unwrap();
            for entry in fs::read_dir(&self.opts.dir)? {
                let p = entry?.path();
                if !done.contains(&p) {
                    let _ = fs::remove_file(p);
                }
            }
            println!(
                "{} swaps, {} kept, worst on_frame call {:.1} ms",
                self.swaps,
                done.len(),
                self.worst_call.as_secs_f64() * 1000.0
            );
            control.stop();
            return Ok(());
        }

        let seg = Duration::from_secs(self.opts.seg_secs);
        let lead = Duration::from_secs(1).min(seg / 2);
        let age = self.seg_start.elapsed();
        let want_cut = self.save.load(Ordering::Relaxed);

        if self.next.is_none() && (age + lead >= seg || want_cut) {
            self.prepare_next();
        }
        // a save request closes the current segment early, but not a tiny one
        if age >= seg || (want_cut && age >= Duration::from_millis(500)) {
            let ready = self.next.as_ref().and_then(|rx| rx.try_recv().ok());
            if let Some((enc, path)) = ready {
                self.swap(enc, path, want_cut);
                if want_cut {
                    self.save.store(false, Ordering::Relaxed);
                }
            }
        }

        self.enc.as_mut().unwrap().send_frame(frame)?;

        self.worst_call = self.worst_call.max(t.elapsed());
        Ok(())
    }
}

pub fn run(opts: Opts, save: Arc<AtomicBool>) -> Result<(), Error> {
    let mon = Monitor::primary()?;
    let size = (mon.width()?, mon.height()?);

    let settings = Settings::new(
        mon,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        (size, opts, save),
    );
    Rolling::start(settings)?;
    Ok(())
}
