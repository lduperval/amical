//! Accessibility context — graceful degradation on Wayland.
//!
//! Wayland's isolation model prevents a client from inspecting other clients'
//! windows or focused widgets; there is no portable equivalent of the macOS
//! AX tree or Windows UIA for out-of-process inspection (AT-SPI2 only exposes
//! apps that opt in, and querying it for the *focused* third-party window is
//! unreliable across desktops). Per the spec, this module returns a null
//! context so the desktop falls back to its generic behavior, and reports
//! permissions as granted since there is nothing to request.

use serde_json::{json, Value};

pub fn get_accessibility_context() -> Value {
    json!({ "context": null })
}

pub fn get_accessibility_status() -> Value {
    json!({ "hasPermission": true, "isEnabled": true })
}

pub fn request_accessibility_permission() -> Value {
    json!({ "granted": true })
}

pub fn get_accessibility_tree_details() -> Value {
    json!({ "tree": null })
}
