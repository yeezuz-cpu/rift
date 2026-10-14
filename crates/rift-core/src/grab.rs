use std::path::PathBuf;

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::ImageFormat;
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

struct OneShot {
    out: PathBuf,
}

impl GraphicsCaptureApiHandler for OneShot {
    type Flags = PathBuf;
    type Error = Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { out: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        frame.save_as_image(&self.out, ImageFormat::Png)?;
        control.stop();
        Ok(())
    }
}

/// Saves a single frame of the primary monitor to `out` as a PNG.
pub fn primary_monitor_to_png(out: impl Into<PathBuf>) -> Result<(), Error> {
    let monitor = Monitor::primary()?;
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        out.into(),
    );
    OneShot::start(settings)?;
    Ok(())
}
