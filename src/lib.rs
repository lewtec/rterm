pub mod attach;
pub mod backend;
pub mod catalog;
pub mod click;
pub mod codec;
pub mod driver;
pub mod hud;
pub mod kitty;
pub mod link;
pub mod notch;
pub mod paste;
pub mod paths;
pub mod place;
pub mod progress;
pub mod store;
pub mod trace;
pub mod uuidv7;
pub mod watchdog;

pub fn version() -> &'static str {
    option_env!("RTERM_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}
