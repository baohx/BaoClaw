// Bridge remote system - remote session management

pub mod manager;

pub use manager::{
    BridgeConfig, BridgeError, BridgeManager, SessionHandle, SessionStatus, SpawnMode,
    WorkAssignment,
};
