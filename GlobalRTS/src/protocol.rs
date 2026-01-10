//! # Protocol
//! 
//! Message types for communication between GlobalUI, Server, and Devices.
//! 
//! All messages are JSON. Simple, human-readable, debuggable.
//! Any AI or human can inspect WebSocket traffic and understand it immediately.

use serde::{Deserialize, Serialize};

// ============================================================================
// DEVICE → SERVER MESSAGES
// ============================================================================

/// Device registration. Sent once when device connects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterMessage {
    pub device_id: String,
    pub device_type: String,  // "robot", "phone", "drone", "iot"
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub altitude: f64,
}

/// Telemetry update. Sent frequently (every 100ms - 1s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryMessage {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub altitude: f64,
    #[serde(default)]
    pub heading: f64,
    #[serde(default)]
    pub speed: f64,
    #[serde(default)]
    pub battery: f64,
    #[serde(default)]
    pub sensors: serde_json::Value,
}

/// Command acknowledgment. Device confirms receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandAck {
    pub command_id: String,
    pub status: String,  // "received", "executing", "completed", "failed"
    #[serde(default)]
    pub result: serde_json::Value,
}

// ============================================================================
// SERVER → DEVICE MESSAGES
// ============================================================================

/// Command to execute. Sent from GlobalUI via server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub command_id: String,
    pub command_type: String,  // "navigate", "stop", "ring", "photo", etc.
    #[serde(default)]
    pub payload: serde_json::Value,
}

// ============================================================================
// GLOBALUI → SERVER MESSAGES
// ============================================================================

/// Request device list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDevices {}

/// Send command to a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendCommand {
    pub device_id: String,
    pub command_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

// ============================================================================
// SERVER → GLOBALUI MESSAGES  
// ============================================================================

/// Device info for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub status: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub heading: f64,
    pub speed: f64,
    pub battery: f64,
    pub last_seen: i64,  // Unix timestamp
}

// ============================================================================
// ENVELOPE
// ============================================================================

/// All messages are wrapped in this envelope.
/// { "type": "telemetry", "data": { ... } }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl Envelope {
    pub fn new<T: Serialize>(msg_type: &str, data: &T) -> Self {
        Self {
            msg_type: msg_type.to_string(),
            data: serde_json::to_value(data).unwrap_or_default(),
        }
    }
    
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}
