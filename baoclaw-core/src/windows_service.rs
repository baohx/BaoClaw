//! Windows Service integration for BaoClaw daemon.
//!
//! On Windows, the daemon can run as a managed service (via sc.exe or the
//! install script). On Linux/macOS, this module compiles to nothing.

#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

/// Name of the Windows service (registered with sc.exe).
pub const SERVICE_NAME: &str = "BaoClawDaemon";
/// Display name shown in services.msc.
pub const SERVICE_DISPLAY_NAME: &str = "BaoClaw AI Coding Assistant";
/// Description shown in services.msc.
pub const SERVICE_DESCRIPTION: &str = "Long-running daemon for BaoClaw. Provides IPC, session management, and tool execution for all BaoClaw clients (CLI, Web, Telegram, etc.).";

/// Flag to signal the daemon to stop (set by SCM on SERVICE_CONTROL_STOP).
static SHUTDOWN_REQUESTED: std::sync::OnceLock<Arc<Mutex<bool>>> = std::sync::OnceLock::new();

/// Check if Windows SCM has requested shutdown.
pub fn is_shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| *g)
        .unwrap_or(false)
}

/// Entry point called by Windows SCM when the service starts.
pub fn windows_service_main(_arguments: &[OsString]) {
    // Initialize shutdown flag
    let shutdown = Arc::new(Mutex::new(false));
    let _ = SHUTDOWN_REQUESTED.set(shutdown.clone());

    // Register service control handler
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Interrogate => {
                if let ServiceControl::Stop = control_event {
                    if let Ok(mut g) = shutdown.lock() {
                        *g = true;
                    }
                }
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = match service_control_handler::register(SERVICE_NAME, event_handler) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to register service control handler: {}", e);
            return;
        }
    };

    // Report Running
    let _ = status_handle.report_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    });

    // Run the daemon main logic.
    // The daemon's shutdown monitor (spawned in main) periodically calls
    // is_shutdown_requested() and gracefully persists + exits when true.
    crate::run_daemon_main_with_shutdown_check();

    // Report Stopped
    let _ = status_handle.report_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    });
}

/// Dispatch this process as a Windows service.
/// Called from main() when --run-as-service flag is present.
pub fn dispatch_as_service() -> Result<(), Box<dyn std::error::Error>> {
    service_dispatcher::start(SERVICE_NAME, windows_service_main)
        .map_err(|e| format!("Failed to dispatch service: {}", e).into())
}

/// Register (install) the service using sc.exe.
///
/// Creates a Windows service with:
/// - binPath: `"path/to/baoclaw-core.exe" --run-as-service`
/// - start=auto (auto-start on boot)
/// - Display name and description set for services.msc
pub fn install_service() -> Result<(), Box<dyn std::error::Error>> {
    // Get current executable path
    let exe_path = std::env::current_exe()?;
    let exe_str = exe_path.to_string_lossy().to_string();

    println!("Installing BaoClaw Windows Service...");
    println!("Executable: {}", exe_str);

    // Use sc.exe to create the service (simplest cross-version approach)
    let bin_path = format!("\"{}\" --run-as-service", exe_str);
    let create_output = std::process::Command::new("sc")
        .args([
            "create",
            SERVICE_NAME,
            "binPath=",
            &bin_path,
            "DisplayName=",
            SERVICE_DISPLAY_NAME,
            "start=",
            "auto",
        ])
        .output()?;

    if !create_output.status.success() {
        let stderr = String::from_utf8_lossy(&create_output.stderr);
        let stdout = String::from_utf8_lossy(&create_output.stdout);
        return Err(format!("sc create failed:\nstdout: {}\nstderr: {}", stdout, stderr).into());
    }

    // Set description
    let _ = std::process::Command::new("sc")
        .args(["description", SERVICE_NAME, SERVICE_DESCRIPTION])
        .output();

    println!("✓ Service installed successfully.");
    println!();
    println!("To start it now:  net start {}", SERVICE_NAME);
    println!("Or via services.msc (search 'services' in Start menu).");
    println!("The service will auto-start on boot (start=auto).");

    Ok(())
}

/// Unregister (uninstall) the service.
///
/// Stops the service if running, then deletes it via sc.exe.
pub fn uninstall_service() -> Result<(), Box<dyn std::error::Error>> {
    println!("Uninstalling BaoClaw Windows Service...");

    // Stop first (ignore error if not running)
    let _ = std::process::Command::new("net")
        .args(["stop", SERVICE_NAME])
        .output();

    // Brief delay to let the service actually stop
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Delete
    let delete_output = std::process::Command::new("sc")
        .args(["delete", SERVICE_NAME])
        .output()?;

    if !delete_output.status.success() {
        let stderr = String::from_utf8_lossy(&delete_output.stderr);
        let stdout = String::from_utf8_lossy(&delete_output.stdout);
        return Err(format!("sc delete failed:\nstdout: {}\nstderr: {}", stdout, stderr).into());
    }

    println!("✓ Service uninstalled successfully.");
    Ok(())
}
