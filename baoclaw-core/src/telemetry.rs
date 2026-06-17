//! Telemetry module — re-exports from the canonical implementation.
//!
//! **Deprecated**: This module is a thin re-export shim.
//! The canonical telemetry implementation lives in
//! [`crate::engine::telemetry`].
//!
//! Please update your imports to use `crate::engine::telemetry::*` directly.
//!
//! This shim exists for backward compatibility so that existing code
//! referencing `crate::telemetry::*` continues to work.

// Re-export everything from the canonical engine::telemetry module.
pub use crate::engine::telemetry::{
    TelemetryCollector, TelemetryExporter, TrendAnalyzer,
    DailyStats, SessionSnapshot, ToolUsageStat, Trend, TrendDirection, UsageStats,
};

// Also re-export the sub-modules for deep-import compatibility.
pub mod collector {
    pub use crate::engine::telemetry::collector::*;
}
pub mod export {
    pub use crate::engine::telemetry::export::*;
}
pub mod trends {
    pub use crate::engine::telemetry::trends::*;
}
pub mod types {
    pub use crate::engine::telemetry::types::*;
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single telemetry event (legacy in-memory event format).
///
/// **Deprecated**: This type is retained for backward compatibility.
/// For structured telemetry, prefer the types in
/// [`crate::engine::telemetry::types`] such as `UsageStats` and
/// `SessionSnapshot`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub event_type: String,
    pub timestamp: String,
    pub data: Value,
}
