//! Leaf infrastructure shared by `tools` and `engine`.
//!
//! Dependency rule: both `tools` and `engine` may depend on `infra`;
//! `infra` depends on neither (std + external crates only).

pub mod file_cache;
pub mod sandbox_config;
pub mod tool_result_store;
