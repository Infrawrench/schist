//! Editor workspace, canvas interaction, panels and dialogs.
use schist_app_actions as actions;
#[cfg(not(sandboxed))]
mod ai;
#[cfg(sandboxed)]
#[path = "ai_stub.rs"]
mod ai;
use schist_app_fonts as fonts;
#[cfg(target_os = "android")]
use schist_app_platform::android;
#[cfg(not(sandboxed))]
use schist_app_platform::drag_out;
#[cfg(target_arch = "wasm32")]
use schist_app_platform::web;
#[cfg(not(target_arch = "wasm32"))]
use schist_app_services::telemetry;
use schist_app_services::{crash, update};
use schist_app_settings::feature_enabled;
#[cfg(not(target_arch = "wasm32"))]
use schist_video as video;
mod color_picker;
mod curve_editor;
mod dialogs;
mod gallery;
pub mod keymap;
pub mod native_menu;
mod panels;
mod style_dialog;
mod ui;
pub mod workspace;
pub use workspace::Workspace;
