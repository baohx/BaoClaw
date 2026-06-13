//! Multi-Model Router — automatic model selection based on task characteristics.
//!
//! This module provides intelligent routing of prompts to the most appropriate
//! AI model based on configurable rules, budget constraints, and learned patterns.
//!
//! # Architecture
//!
//! - `types` - Core data types: `ModelInfo`, `RoutingRule`, `RouteCondition`, `RoutingDecision`
//! - `router` - The `ModelRouter` engine that evaluates rules and selects models
//! - `budget` - `BudgetManager` for tracking daily/monthly spending limits
//! - `learning` - `RouterLearning` for recording decisions and optimizing rules over time
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use baoclaw_core::engine::model_router::ModelRouter;
//!
//! let router = ModelRouter::new();
//! let decision = router.route("Write a sorting function", 0, 0.2);
//! println!("Using model: {}", decision.selected_model);
//! ```
//!
//! # Configuration
//!
//! Routing configuration is stored in `~/.baoclaw/model_routing.json`:
//!
//! ```json
//! {
//!   "rules": [
//!     {
//!       "id": "simple-to-haiku",
//!       "description": "Simple tasks use Haiku",
//!       "condition": { "type": "task_complexity", "params": { "min": 0.0, "max": 0.3 } },
//!       "target_model": "claude-3-5-haiku-20241022",
//!       "priority": 100,
//!       "enabled": true
//!     }
//!   ],
//!   "default": "claude-sonnet-4-20250514"
//! }
//! ```
//!
//! Budget state persists to `~/.baoclaw/budget.json`.
//! Routing statistics are stored in `~/.baoclaw/router_stats.db` (SQLite).

pub mod budget;
pub mod learning;
pub mod router;
pub mod types;

// Re-export main types for convenience
