use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
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

pub struct Opts {
    pub dir: PathBuf,
    pub seg_secs: u64,
    pub keep: usize,
    pub run_secs: u64,
    pub fps: u32,
}

struct Rolling {
    opts: Opts,
    size: (u32, u32),
    enc: Option<VideoEncoder>,
    cur: PathBuf,
    done: VecDeque<PathBuf>,
    idx: u32,
    seg_start: Instant,
    started: Instant,
    gaps: Vec<Duration>,
}

impl Rolling {
    fn open(&mut self) -> Result<(), Error> {
        self.cur = self.opts.dir.join(format!("seg_{:03}.mp4", self.idx));
        self.idx += 1;
        self.enc = Some(VideoEncoder::new(
            VideoSettingsBuilder::new(self.size.0, self.size.1)
                .sub_type(VideoSettingsSubType::H264)
                .frame_rate(self.opts.fps),
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            &self.cur,
        )?);
        self.seg_start = Instant::now();
        Ok(())
    }

    fn rotate(&mut self) -> Result<(), Error> {
        let t = Instant::now();
        self.enc.take().unwrap().finish()?;
        self.done.push_back(self.cur.clone());
        while self.done.len() > self.opts.keep {
            let old = self.done.pop_front().unwrap();
            let _ = fs::remove_file(old);
        }
        self.open()?;
        self.gaps.push(t.elapsed());
        Ok(())
    }
}

impl GraphicsCaptureApiHandler for Rolling {
    type Flags = ((u32, u32), Opts);
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (size, opts) = ctx.flags;
        let _ = fs::remove_dir_all(&opts.dir);
        fs::create_dir_all(&opts.dir)?;

        let mut r = Self {
            opts,
            size,
            enc: None,
            cur: PathBuf::new(),
            done: VecDeque::new(),
            idx: 0,
            seg_start: Instant::now(),
            started: Instant::now(),
            gaps: Vec::new(),
        };
        r.open()?;
        Ok(r)
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.started.elapsed() >= Duration::from_secs(self.opts.run_secs) {
            self.enc.take().unwrap().finish()?;
            self.done.push_back(self.cur.clone());
            let worst = self.gaps.iter().max().copied().unwrap_or_default();
            println!(
                "{} segments rotated, worst rotate {:.0} ms, {} kept",
                self.gaps.len(),
                worst.as_secs_f64() * 1000.0,
                self.done.len()
            );
            control.stop();
            return Ok(());
        }

        if self.seg_start.elapsed() >= Duration::from_secs(self.opts.seg_secs) {
            self.rotate()?;
        }
        self.enc.as_mut().unwrap().send_frame(frame)?;
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
