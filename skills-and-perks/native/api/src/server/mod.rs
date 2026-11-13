//! The server half: data files, player state, work done on the server tick.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod actions;
pub mod commands;
pub mod data;
pub mod effects;
pub mod enforce;
pub mod queue;
pub mod state;
#[cfg(test)]
pub mod test_host;
pub mod text;

/// Locks a mutex, going on with the data a panicking holder left: the game keeps running after a hook panics.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
