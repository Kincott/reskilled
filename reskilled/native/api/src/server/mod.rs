//! The server half: data files, player state, work done on the server tick.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod actions;
pub mod commands;
pub mod data;
pub mod earning;
pub mod effects;
pub mod enforce;
pub mod focus;
pub mod queue;
pub mod rules;
pub mod sharing;
pub mod state;

/// Locks a mutex, going on with the data a panicking holder left: the game keeps running after a hook panics.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A uuid as vanilla commands take it, written with hyphens.
pub(crate) fn uuid_text(uuid: u128) -> String {
    let hex = format!("{uuid:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The server stopped: forget everything kept for its world (rules in force, waiting requests, placed blocks,
/// invites, players online), so the next world opened in this game plays by its own. The loaded data files are
/// read again when the next server starts.
pub fn forget_world() {
    actions::reset();
    earning::reset();
    effects::reset();
    enforce::reset();
    focus::reset();
    queue::reset();
    rules::reset();
    sharing::reset();
    state::reset();
}
