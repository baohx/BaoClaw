//! Memory management module.
//!
//! This module provides long-term memory storage and retrieval for BaoClaw.
//! It includes:
//! - `MemoryStore`: Persistent storage for facts, preferences, and decisions
//! - `DecayConfig`: Configuration for memory importance decay
//! - `MemoryArchive`: Storage for archived low-importance memories
//! - `decay`: Automatic importance scoring and archival functions
//! - `archive`: Archive management for low-importance memories
//! - `cleanup`: Periodic cleanup scheduler for memory maintenance

pub mod archive;
pub mod cleanup;
pub mod decay;
pub mod store;

// Re-export commonly used types for convenience
pub use archive::MemoryArchive;
pub use cleanup::MemoryCleanupScheduler;
pub use decay::{apply_decay, DecayConfig};
pub use store::{parse_category, MemoryEntry, MemoryStore};
