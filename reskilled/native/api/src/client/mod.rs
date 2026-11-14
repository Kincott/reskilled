//! The client half: the definitions and requirements the server sends, tooltips, and the skills screen.

pub mod badge;
pub mod cache;
pub mod keys;
pub mod layout;
pub mod options;
pub mod paint;
#[cfg(test)]
mod preview;
#[expect(dead_code, reason = "the guide screen draws it, in a later task")]
pub mod scene;
pub mod screen;
pub mod settings;
pub mod strip;
pub mod tooltip;
pub mod warning;
