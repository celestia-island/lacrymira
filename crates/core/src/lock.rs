//! Pessimistic locking modes.

use serde::{Deserialize, Serialize};

/// Lock mode for SELECT ... FOR UPDATE family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockMode {
    /// `FOR UPDATE` — exclusive lock, blocks other FOR UPDATE / FOR SHARE
    Update,
    /// `FOR UPDATE NOWAIT` — fail immediately (55P03) if row is locked
    UpdateNoWait,
    /// `FOR UPDATE SKIP LOCKED` — skip locked rows (job queue pattern)
    SkipLocked,
    /// `FOR SHARE` — shared lock, blocks FOR UPDATE but allows concurrent FOR SHARE
    Share,
}

impl LockMode {
    /// The SQL clause this mode renders to.
    pub fn sql(&self) -> &'static str {
        match self {
            LockMode::Update => "FOR UPDATE",
            LockMode::UpdateNoWait => "FOR UPDATE NOWAIT",
            LockMode::SkipLocked => "FOR UPDATE SKIP LOCKED",
            LockMode::Share => "FOR SHARE",
        }
    }
}
