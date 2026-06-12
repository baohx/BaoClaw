//! Agent Hooks System
//!
//! This module provides a hook system that allows automatic execution of actions
//! when specific events occur in BaoClaw.
//!
//! # Architecture
//!
//! - `triggers` - Defines the types of events that can trigger hooks
//! - `actions` - Defines the actions that hooks can execute
//! - `manager` - Manages hook registration, loading, and execution
//!
//! # Configuration
//!
//! Hooks are configured in `~/.baoclaw/hooks.json` with the following format:
//!
//! ```json
//! {
//!   "hooks": [
//!     {
//!       "id": "auto-lint-on-save",
//!       "name": "Auto Lint on Save",
//!       "trigger": "file_edited",
//!       "filter": { "file_pattern": "*.ts" },
//!       "action": { "type": "run_command", "command": "npm run lint --fix {file}" },
//!       "enabled": true
//!     }
//!   ]
//! }
//! ```

pub mod actions;
pub mod manager;
pub mod triggers;

pub use actions::{Action, ActionExecutor, ActionResult};
pub use manager::{Hook, HookManager, HookManagerConfig};
pub use triggers::{Filter, Trigger, TriggerContext, TriggerType};
