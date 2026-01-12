//! # Protocol
//! 
//! Message types for communication between GlobalUI, Server, and Devices.
//! 
//! All messages are JSON. Simple, human-readable, debuggable.
//! Any AI or human can inspect WebSocket traffic and understand it immediately.
//!
//! ## Device Connection Flow
//! 
//! 1. Device calls POST /api/pair/request → Gets "pending" status
//! 2. Server generates 6-digit code, shows in GlobalUI
//! 3. User tells device the code (verbally, or device shows prompt)
//! 4. Device calls POST /api/pair/confirm with code → Gets auth token
//! 5. Device stores token locally
//! 6. Device connects WebSocket, sends "register" with token
//! 7. Server validates token, confirms registration
//! 8. Device starts sending telemetry

use serde::{Deserialize, Serialize};

// ============================================================================
// DEVICE → SERVER MESSAGES
// ============================================================================

/// Device registration. Sent once when device connects via WebSocket.
/// Token is required - obtained via /api/pair/confirm endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterMessage {
    /// Auth token from pairing process (required)
    #[serde(default)]
    pub token: Option<String>,
    
    /// Device unique identifier
    pub device_id: String,
    
    /// Device type: robot, phone, drone, sensor, etc.
    pub device_type: String,
    
    /// Human-readable name
    pub name: String,
    
    /// Initial GPS latitude
    pub latitude: f64,
    
    /// Initial GPS longitude  
    pub longitude: f64,
    
    /// Initial altitude (meters above sea level)
    #[serde(default)]
    pub altitude: f64,
    
    /// Device capabilities (optional, for future use)
    #[serde(default)]
    pub capabilities: Vec<String>,
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

// ============================================================================
// GLOBALUI → SERVER MESSAGES
// ============================================================================

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
    pub last_seen: i64,
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

// ============================================================================
// MESSAGE TYPE REFERENCE
// ============================================================================
//
// Device → Server:
//   - register: Device connects with token
//   - telemetry: Position/sensor updates
//   - command:ack: Acknowledges receipt of command
//   - command:complete: Command finished executing
//
// Server → Device:
//   - registered: Confirms registration
//   - error: Authentication/other errors
//   - command: Execute a command
//
// UI → Server:
//   - getDevices: Request list of all devices
//   - sendCommand: Send command to a device
//   - dismissPairing: Dismiss/reject a pairing request
//   - revokeDevice: Remove a device from the system
//
// Server → UI:
//   - devices:list: Full list of devices
//   - device:online: Device connected
//   - device:offline: Device disconnected
//   - device:update: Telemetry update
//   - device:revoked: Device was removed
//   - pairing:requests: List of pending pairing requests
//   - command:sent: Command was sent to device
//   - command:ack: Device acknowledged command
//   - command:complete: Device completed command
