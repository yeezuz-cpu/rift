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
    pub path: String,
    pub secs: u64,
    pub fps: u32,
}

struct Recorder {
    enc: Option<VideoEncoder>,
    start: Instant,
    limit: Duration,
    frames: u32,
    busy: Duration,
}

impl GraphicsCaptureApiHandler for Recorder {
    type Flags = (u32, u32, Opts);
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (w, h, opts) = ctx.flags;
        let enc = VideoEncoder::new(
            VideoSettingsBuilder::new(w, h)
                .sub_type(VideoSettingsSubType::H264)
                .frame_rate(opts.fps),
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            &opts.path,
        )?;
        Ok(Self {
            enc: Some(enc),
            start: Instant::now(),
            limit: Duration::from_secs(opts.secs),
            frames: 0,
            busy: Duration::ZERO,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let t = Instant::now();
        self.enc.as_mut().unwrap().send_frame(frame)?;
        self.busy += t.elapsed();
        self.frames += 1;

        if self.start.elapsed() >= self.limit {
            self.enc.take().unwrap().finish()?;
            let secs = self.start.elapsed().as_secs_f64();
            println!(
                "{} frames in {:.1}s ({:.1} fps), avg send_frame {:.2} ms",
                self.frames,
                secs,
                self.frames as f64 / secs,
                self.busy.as_secs_f64() * 1000.0 / self.frames as f64,
            );
            control.stop();
        }
        Ok(())
    }
}

pub fn record_primary(opts: Opts) -> Result<(), Error> {
    let mon = Monitor::primary()?;
    let (w, h) = (mon.width()?, mon.height()?);

    let settings = Settings::new(
        mon,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        (w, h, opts),
    );
    Recorder::start(settings)?;
    Ok(())
}
