//! Auction-house price data read from other add-ons' SavedVariables.
//!
//! Each supported add-on gets its own module that knows its SavedVariables
//! file name and how to pull a normalized [`price::PriceTable`] out of it.
//! Adding another add-on later means adding one more module here, not
//! touching the recommendation engine that consumes [`price::PriceTable`].

pub mod auctionator;
mod lua_raw;
pub mod price;

pub use price::{PriceRecord, PriceTable};

/// One supported AH add-on, named for detection and settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Auctionator,
}

impl Source {
    pub const ALL: &'static [Source] = &[Source::Auctionator];

    pub fn id(&self) -> &'static str {
        match self {
            Source::Auctionator => "auctionator",
        }
    }

    pub fn data_file(&self) -> &'static str {
        match self {
            Source::Auctionator => auctionator::DATA_FILE,
        }
    }
}

/// Reads the given add-on's price tables, keyed by realm name (an account's
/// SavedVariables cover every realm it has played on).
pub fn load_all(saved_variables_dir: &std::path::Path, source: Source) -> std::collections::HashMap<String, PriceTable> {
    // Read as bytes, not a `String`: these files can hold binary data (an
    // unescaped CBOR blob) inside an otherwise-text Lua literal, so the file
    // as a whole isn't reliably valid UTF-8.
    let Ok(src) = std::fs::read(saved_variables_dir.join(source.data_file())) else {
        return std::collections::HashMap::new();
    };
    match source {
        Source::Auctionator => auctionator::parse_all(&src),
    }
}
