# Linux Helper (Wayland) Specification

This document defines the architecture, JSON-RPC protocol, and implementation plan for the `linux-helper`, a native Wayland client required to support the desktop application on Linux.

---

## 1. JSON-RPC Protocol Definition

The helper acts as a standard UNIX filter, reading JSON-RPC 2.0 requests from `stdin` and writing responses (and unsolicited events) to `stdout`. Each message is a single line terminated by a newline (`\n`).

### Message Envelopes

**Request (Desktop -> Helper)**

```json
{
  "id": "uuid-string",
  "method": "methodName",
  "params": {}
}
```

**Response (Helper -> Desktop)**

```json
{
  "id": "uuid-string",
  "result": {},
  "error": { "code": 123, "message": "Error details", "data": null }
}
```

**Unsolicited Event (Helper -> Desktop)**

```json
{
  "type": "keyDown",
  "payload": {
    "key": "a",
    "code": "KeyA",
    "keyCode": 38,
    "altKey": false,
    "ctrlKey": false,
    "shiftKey": false,
    "metaKey": false
  },
  "timestamp": "2026-08-23T11:57:00Z"
}
```

Supported event types: `keyDown`, `keyUp`, `flagsChanged`, `activeDisplayChanged`, `widgetWindowMoved` (Linux; payload `{ title, x, y, width, height }`).

### Required RPC Methods

| Method                           | Params                             | Result                                                                                                                                                           | Description                                                                                                                                                                        |
| -------------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `getAccessibilityContext`        | `{ "editableOnly": boolean }`      | `{ "context": AppContext \| null }`                                                                                                                              | Retrieves the current focused UI element and text selection. _(Wayland isolates clients; this may require compositor-specific DBus APIs or xdg-desktop-portal, else return null)._ |
| `getAccessibilityStatus`         | `{}`                               | `{ "hasPermission": bool, "isEnabled": bool }`                                                                                                                   | Checks if the helper has the necessary compositor permissions.                                                                                                                     |
| `requestAccessibilityPermission` | `{}`                               | `{ "granted": bool }`                                                                                                                                            | Prompts the user for permission (e.g., via portal).                                                                                                                                |
| `pasteText`                      | `{ "text": "string" }`             | `{ "success": bool }`                                                                                                                                            | Injects text into the clipboard and simulates a paste action.                                                                                                                      |
| `getSelectedTextViaCopy`         | `{}`                               | `{ "text": "string" \| null }`                                                                                                                                   | Simulates a copy action and reads the clipboard.                                                                                                                                   |
| `startRecording`                 | `{ "deviceId": string \| null }`   | `{ "success": bool }`                                                                                                                                            | Starts audio capture.                                                                                                                                                              |
| `stopRecording`                  | `{}`                               | `{ "success": bool, "path": string }`                                                                                                                            | Stops audio capture and returns the file path.                                                                                                                                     |
| `setShortcuts`                   | `{ "shortcuts": [...] }`           | `{ "success": bool }`                                                                                                                                            | Registers global hotkeys.                                                                                                                                                          |
| `setDraftEnterCapture`           | `{ "enabled": bool }`              | `{ "success": bool }`                                                                                                                                            | Toggles interception of the Enter key.                                                                                                                                             |
| `setAllowInjectedKeys`           | `{ "allowed": bool }`              | `{ "success": bool }`                                                                                                                                            | Configures input injection behavior.                                                                                                                                               |
| `recheckPressedKeys`             | `{}`                               | `{ "keys": [...] }`                                                                                                                                              | Returns currently held keys.                                                                                                                                                       |
| `getAccessibilityTreeDetails`    | _(varies)_                         | _(varies)_                                                                                                                                                       | Advanced accessibility tree inspection (best-effort on Linux).                                                                                                                     |
| `getLinuxIntegrationStatus`      | `{}`                               | `{ inputMethod, injectionAvailable, injectionBackend?, clipboardBackend, clipboardStealsFocus, extensionAvailable, extensionVersion?, shellVersion?, message? }` | Linux only. What this helper build can do on the running desktop (see `packages/types`).                                                                                           |
| `placeWidgetWindow`              | `{ title, x, y, above?, sticky? }` | `{ success, found, message? }`                                                                                                                                   | Linux only. Ask the Amical GNOME Shell extension to keep one of Amical's windows above others, on every workspace, at a position. Emits `widgetWindowMoved` events afterwards.     |

---

## 2. Rust Crate Architecture

The `linux-helper` should be implemented in **Rust**, leveraging its strong concurrency model and excellent Wayland ecosystem bindings.

### Dependencies (`Cargo.toml`)

```toml
[package]
name = "linux-helper"
version = "0.1.0"
edition = "2021"

[dependencies]
# IPC & Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["full", "io-util"] }
uuid = { version = "1.0", features = ["v4", "serde"] }

# Wayland & Desktop Integration
wayland-client = "0.31"
smithay-client-toolkit = "0.19"
zbus = "4.0" # For xdg-desktop-portal DBus communication

# Audio
cpal = "0.15" # Or pulse/pipewire specific crates
```

### Project Layout

```
packages/native-helpers/linux-helper/
├── Cargo.toml
└── src/
    ├── main.rs         # Entry point, Tokio runtime, stdin/stdout loop
    ├── rpc.rs          # Serde structs for Requests, Responses, and Events
    ├── wayland.rs      # Wayland connection and event queue dispatch
    ├── clipboard.rs    # wlr-data-control implementation for paste/copy
    ├── shortcuts.rs    # Global shortcuts (via GlobalShortcuts portal or ext-global-shortcuts-v1)
    ├── accessibility.rs# Context extraction (DBus/Portal)
    └── audio.rs        # Audio recording orchestration
```

### Technical Considerations for Wayland

- **Clipboard**: Use the `wlr-data-control-unstable-v1` protocol to manipulate the clipboard without requiring an active surface focus.
- **Input Interception**: Wayland strictly prohibits global input sniffing. You must use the `xdg-desktop-portal` Global Shortcuts API (`org.freedesktop.portal.GlobalShortcuts`) to register hotkeys. Unsolicited key events (`keyDown`, etc.) will only fire for registered shortcuts.
- **Accessibility Context**: Wayland clients cannot inspect other clients' windows. If `getAccessibilityContext` is called, you must either rely on compositor-specific DBus APIs (like KWin scripting) or safely return `null` and fallback to generic behavior in the desktop app.

---

## 3. Implementation Plan for Fable 5

_To the autonomous agent: Follow these steps sequentially to build the `linux-helper`._

### **Phase 1: Project Scaffolding & IPC Loop**

1. Initialize a new Rust binary crate at `packages/native-helpers/linux-helper`.
2. Add `serde`, `serde_json`, and `tokio` to `Cargo.toml`.
3. Create `src/rpc.rs` and define the Serde structs for `RpcRequest`, `RpcResponse`, and `HelperEvent` matching the JSON-RPC spec above.
4. In `src/main.rs`, set up a `tokio` asynchronous stdin reader (using `tokio::io::BufReader`) that reads line-by-line, deserializes requests, routes them to a dummy handler, and writes JSON responses to stdout.

### **Phase 2: Wayland Client & Protocol Bindings**

1. Add `wayland-client` and `smithay-client-toolkit`.
2. Create `src/wayland.rs` to initialize the Wayland connection, registry, and event queue.
3. Hook the Wayland event loop into the `tokio` runtime (e.g., using `wayland-client`'s async support or a dedicated polling thread sending messages via a `tokio::sync::mpsc` channel).

### **Phase 3: Clipboard & Text Extraction**

1. Implement `src/clipboard.rs` using `wlr-data-control` (if available via `smithay-client-toolkit` or manual protocol bindings).
2. Wire up the `pasteText` RPC method: Write the payload to the Wayland clipboard and optionally simulate a generic `Ctrl+V` (if input injection is supported by the compositor).
3. Wire up `getSelectedTextViaCopy`: Simulate a `Ctrl+C` (if permitted) and read the resulting clipboard selection.

### **Phase 4: Global Shortcuts via Portal**

1. Add `zbus` to interact with DBus.
2. Implement `src/shortcuts.rs` using `org.freedesktop.portal.GlobalShortcuts`.
3. Wire up `setShortcuts` to register the requested hotkeys via DBus.
4. Listen for DBus shortcut activation signals and emit the corresponding `HelperEvent` (`keyDown`, `keyUp`) to `stdout`.

### **Phase 5: Audio Recording**

1. Implement `src/audio.rs` using `cpal` to capture the default microphone input.
2. Wire up `startRecording` to begin streaming audio to a temporary file.
3. Wire up `stopRecording` to finalize the file and return its absolute path.

### **Phase 6: Accessibility (Graceful Degradation)**

1. Implement `src/accessibility.rs`.
2. Wire up `getAccessibilityStatus` to return `{ hasPermission: true, isEnabled: true }` (assuming Wayland portals handle permissions natively).
3. Wire up `getAccessibilityContext` to return `{ context: null }` as a baseline, since generic window inspection is denied by Wayland protocol design. Document this limitation in the source.
