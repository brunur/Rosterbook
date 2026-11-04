//! Reads text out of a captured image.
//!
//! The default implementation shells out to the system `tesseract` binary
//! via `rusty-tesseract`, because that's the only interface tesseract's CLI
//! offers. **Caveat for the "no images stored" constraint**: `rusty-tesseract`
//! writes the image to a `tempfile`-crate temp file to hand it to the CLI,
//! and that file is deleted when the wrapper's `Image` value drops (right
//! after this call returns) — nothing persists, but the bytes do briefly
//! touch disk. A pure-Rust OCR engine (e.g. `ocrs`) would avoid that
//! entirely at the cost of bundling model weights; this trait exists so
//! that swap can happen later without touching the rest of the watcher.

use xcap::image::{DynamicImage, RgbaImage};

pub trait OcrEngine {
    fn read_text(&self, image: &RgbaImage) -> anyhow::Result<String>;
}

#[derive(Default)]
pub struct TesseractOcr {
    pub args: rusty_tesseract::Args,
}

impl OcrEngine for TesseractOcr {
    fn read_text(&self, image: &RgbaImage) -> anyhow::Result<String> {
        let dynamic = DynamicImage::ImageRgba8(image.clone());
        let tess_image = rusty_tesseract::Image::from_dynamic_image(&dynamic)?;
        Ok(rusty_tesseract::image_to_string(&tess_image, &self.args)?)
    }
}

#[cfg(test)]
pub mod fakes {
    use super::*;

    /// An OCR engine that returns fixed text (or a fixed error), for tests
    /// that exercise the sampling loop without a real tesseract install.
    pub struct FakeOcr {
        pub result: Result<String, String>,
    }

    impl OcrEngine for FakeOcr {
        fn read_text(&self, _image: &RgbaImage) -> anyhow::Result<String> {
            self.result.clone().map_err(|e| anyhow::anyhow!(e))
        }
    }
}
