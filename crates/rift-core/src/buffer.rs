use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
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

type Error = Box<dyn std::error::Error + Send + Sync>;
type Segment = (VideoEncoder, PathBuf);

pub struct Opts {
    pub dir: PathBuf,
    pub seg_secs: u64,
    pub keep: usize,
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
    enc: Option<VideoEncoder>,
    cur: PathBuf,
    next: Option<Receiver<Segment>>,
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

    fn swap(&mut self, enc: VideoEncoder, path: PathBuf) {
        let old = self.enc.replace(enc).unwrap();
        let old_path = std::mem::replace(&mut self.cur, path);
        let _ = self.to_fin.as_ref().unwrap().send((old, old_path));
        self.next = None;
        self.seg_start = Instant::now();
        self.swaps += 1;
    }
}

impl GraphicsCaptureApiHandler for Rolling {
    type Flags = ((u32, u32), Opts);
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (size, opts) = ctx.flags;
        let _ = fs::remove_dir_all(&opts.dir);
        fs::create_dir_all(&opts.dir)?;

        let done = Arc::new(Mutex::new(VecDeque::new()));
        let (tx, rx) = mpsc::channel::<Segment>();

        let keep = opts.keep;
        let done2 = done.clone();
        let fin_thread = thread::spawn(move || {
            for (enc, path) in rx {
                let _ = enc.finish();
                let mut d = done2.lock().unwrap();
                d.push_back(path);
                while d.len() > keep {
                    let old = d.pop_front().unwrap();
                    let _ = fs::remove_file(old);
                }
            }
        });

        let first = opts.dir.join("seg_000.mp4");
        let enc = make_encoder(size, opts.fps, &first)?;

        Ok(Self {
            opts,
            size,
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

        if self.started.elapsed() >= Duration::from_secs(self.opts.run_secs) {
            let last = self.enc.take().unwrap();
            let _ = self.to_fin.as_ref().unwrap().send((last, self.cur.clone()));
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

        if self.next.is_none() && age + lead >= seg {
            self.prepare_next();
        }
        if age >= seg {
            let ready = self.next.as_ref().and_then(|rx| rx.try_recv().ok());
            if let Some((enc, path)) = ready {
                self.swap(enc, path);
            }
        }

        self.enc.as_mut().unwrap().send_frame(frame)?;

        self.worst_call = self.worst_call.max(t.elapsed());
        Ok(())
    }
}

pub fn run(opts: Opts) -> Result<(), Error> {
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
        (size, opts),
    );
    Rolling::start(settings)?;
    Ok(())
}
