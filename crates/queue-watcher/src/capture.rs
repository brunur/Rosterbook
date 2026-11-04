//! Screen capture: grabs one configured region into memory. Implementations
//! must never write the image to disk themselves — see `ocr.rs` for the one
//! documented exception, where the OCR *engine* (not this module) needs a
//! file path.

use alt_ledger_queue::CaptureRegion;
use xcap::image::RgbaImage;
use xcap::Monitor;

pub trait ScreenCapturer {
    fn capture(&self, region: CaptureRegion) -> anyhow::Result<RgbaImage>;
}

pub struct XcapCapturer;

impl ScreenCapturer for XcapCapturer {
    fn capture(&self, region: CaptureRegion) -> anyhow::Result<RgbaImage> {
        let monitors = Monitor::all()?;
        let monitor = monitors
            .get(region.monitor_index)
            .ok_or_else(|| anyhow::anyhow!("no monitor at index {}", region.monitor_index))?;
        Ok(monitor.capture_region(region.x, region.y, region.width, region.height)?)
    }
}

#[cfg(test)]
pub mod fakes {
    use super::*;

    /// A capturer that returns a fixed image (or a fixed error), for tests
    /// that exercise the sampling loop without a real display.
    pub struct FakeCapturer {
        pub result: Result<RgbaImage, String>,
    }

    impl ScreenCapturer for FakeCapturer {
        fn capture(&self, _region: CaptureRegion) -> anyhow::Result<RgbaImage> {
            self.result.clone().map_err(|e| anyhow::anyhow!(e))
        }
    }
}
