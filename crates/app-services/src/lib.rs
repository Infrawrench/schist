//! Crash reporting, diagnostics and desktop release updates.
pub mod crash;
#[cfg(not(target_arch = "wasm32"))]
pub mod telemetry;
#[cfg(not(sandboxed))]
pub mod update;
#[cfg(sandboxed)]
#[path = "update_stub.rs"]
pub mod update;
