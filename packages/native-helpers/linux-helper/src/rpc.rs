//! Serde models for the JSON-RPC stdin/stdout protocol.
//!
//! These mirror the Zod schemas in `packages/types/src/schemas` (the source of
//! truth shared with SwiftHelper and WindowsHelper). The desktop validates every
//! response against those schemas, so field names and shapes here must match
//! them exactly.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn success(id: &str, result: impl Serialize) -> Self {
        Self {
            id: id.to_string(),
            result: Some(serde_json::to_value(result).expect("result serializes")),
            error: None,
        }
    }

    pub fn failure(id: &str, code: i64, message: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

// JSON-RPC 2.0 error codes used by all helpers.
pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

// ---------------------------------------------------------------------------
// Method params
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteTextParams {
    pub transcript: String,
    #[serde(default)]
    pub preserve_clipboard: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRecordingParams {
    pub mute_system_audio: bool,
    #[serde(default)]
    pub mute_sounds: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRecordingParams {
    pub was_muted: bool,
    #[serde(default)]
    pub mute_sounds: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetShortcutsParams {
    pub subset_chords: Vec<Vec<u32>>,
    pub exact_chords: Vec<Vec<u32>>,
}

#[derive(Debug, Deserialize)]
pub struct SetDraftEnterCaptureParams {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetAllowInjectedKeysParams {
    #[allow(dead_code)] // Windows-only behavior; accepted and ignored on Linux.
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecheckPressedKeysParams {
    pub pressed_key_codes: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Method results
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SuccessResult {
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct SuccessMessageResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSelectedTextViaCopyResult {
    pub selected_text: Option<String>,
    pub clipboard_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecheckPressedKeysResult {
    pub stale_key_codes: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Unsolicited events (helper -> desktop)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyEventPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub key_code: u32,
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub meta_key: bool,
}

#[derive(Debug, Serialize)]
pub struct HelperEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str, // "keyDown" | "keyUp"
    pub payload: KeyEventPayload,
    pub timestamp: String, // ISO 8601
}

pub fn iso_timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
