//! Alt Ledger login-queue tracking: turns the queue watcher's raw OCR text
//! into structured samples, the shapes a queue session is built from, and
//! the on-disk watcher config both the watcher process and the desktop app
//! need to agree on. No screen capture, OCR engine, estimate math, storage
//! or UI lives here — see later crates/modules for those.

pub mod config;
pub mod model;
pub mod parser;

pub use config::{CaptureRegion, WatcherConfig};
pub use model::{EndReason, QueueSample, QueueSession};
pub use parser::{parse, ParsedQueueText};
