//! Alt Ledger core: reads the in-game add-on's saved data and handles
//! settings, game detection and add-on installation. No network code here.

pub mod addon;
pub mod detect;
pub mod lua;
pub mod model;
pub mod settings;

pub use model::{load_game_data, GameData};
pub use settings::Settings;
