//! Accessibility context — graceful degradation on Wayland.
//!
//! Wayland's isolation model prevents a client from inspecting other clients'
//! windows or focused widgets; there is no portable equivalent of the macOS
//! AX tree or Windows UIA for out-of-process inspection (AT-SPI2 only exposes
//! apps that opt in, and querying it for the *focused* third-party window is
//! unreliable across desktops).
//!
//! With the Amical GNOME Shell extension running, the shell tells us which
//! window has keyboard focus. That is enough for app-aware formatting: the
//! desktop matches `application.bundleIdentifier` against the app catalog
//! (macOS bundle ids and Windows process names), so we report the window's
//! WM class / Wayland app id lower-cased, the way Windows reports a process
//! name. Text selection and focused-element details stay null.
//!
//! Without the extension this module returns a null context so the desktop
//! falls back to its generic behavior, and reports permissions as granted
//! since there is nothing to request.

use linux_helper::gnome_shell::GnomeShell;
use serde_json::{json, Value};

pub async fn get_accessibility_context(shell: Option<&GnomeShell>) -> Value {
    let Some(shell) = shell else {
        return json!({ "context": null });
    };
    let Some(window) = shell.focused_window().await else {
        return json!({ "context": null });
    };
    match context_from_focused_window(&window) {
        Some(context) => json!({ "context": context }),
        None => json!({ "context": null }),
    }
}

/// Build the desktop's AppContext (schema 2.0) from the extension's focused
/// window description. Returns None when the description carries no usable
/// identity.
pub fn context_from_focused_window(window: &Value) -> Option<Value> {
    let text = |key: &str| {
        window
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let identifier = text("sandboxedAppId")
        .or_else(|| text("gtkApplicationId"))
        .or_else(|| text("wmClass"))
        .or_else(|| text("wmClassInstance"))?;
    let name = text("wmClass")
        .or_else(|| text("title"))
        .unwrap_or(identifier);
    let pid = window.get("pid").and_then(Value::as_i64).unwrap_or(0);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Some(json!({
        "schemaVersion": "2.0",
        "application": {
            "name": name,
            "bundleIdentifier": identifier.to_lowercase(),
            "version": null,
            "pid": pid,
        },
        "windowInfo": {
            "title": text("title"),
            "url": null,
        },
        "focusedElement": null,
        "textSelection": null,
        "timestamp": timestamp,
        "metrics": {
            "totalTimeMs": 0,
            "textMarkerAttempted": false,
            "textMarkerSucceeded": false,
            "fallbacksUsed": [],
            "errors": [],
            "timedOut": false,
            "webAreaRetryAttempted": false,
            "webAreaFound": false,
            "webAreaRetrySucceeded": false,
        },
    }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_window_becomes_an_app_context_keyed_by_wm_class() {
        let context = context_from_focused_window(&json!({
            "title": "Inbox - Betterbird",
            "wmClass": "Betterbird",
            "wmClassInstance": "Navigator",
            "gtkApplicationId": "",
            "sandboxedAppId": "",
            "pid": 4242,
            "clientType": "x11",
        }))
        .unwrap();
        assert_eq!(context["schemaVersion"], "2.0");
        assert_eq!(context["application"]["bundleIdentifier"], "betterbird");
        assert_eq!(context["application"]["name"], "Betterbird");
        assert_eq!(context["application"]["pid"], 4242);
        assert_eq!(context["windowInfo"]["title"], "Inbox - Betterbird");
        assert!(context["textSelection"].is_null());
    }

    #[test]
    fn sandboxed_app_id_wins_and_empty_identity_yields_no_context() {
        let context = context_from_focused_window(&json!({
            "title": "Mail",
            "wmClass": "betterbird",
            "sandboxedAppId": "eu.betterbird.Betterbird",
            "pid": 1,
        }))
        .unwrap();
        assert_eq!(
            context["application"]["bundleIdentifier"],
            "eu.betterbird.betterbird"
        );
        assert!(context_from_focused_window(&json!({ "title": "x", "pid": 1 })).is_none());
    }
}
