//! # State Management
//! 
//! SQLite-based state storage for device registry, pairing, and commands.
//! 
//! WHY SQLITE:
//! - It's a file. No database server. No connection management.
//! - Embedded in binary via rusqlite's "bundled" feature.
//! - 20+ years of backwards compatibility.
//! - Any tool can inspect the database file.
//! 
//! TABLES:
//! - devices: Registered devices and their current state
//! - pairing_requests: Pending 6-digit code pairing requests
//! - commands: Command queue and history
//! 
//! Telemetry (high-volume time-series) goes to flat files instead.

use rusqlite::{Connection, params};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::protocol::DeviceInfo;

/// Thread-safe database handle.
pub struct StateDb {
    conn: Arc<Mutex<Connection>>,
}

/// Pairing request info
#[derive(Debug, Clone)]
pub struct PairingRequest {
    pub device_id: String,
    pub name: String,
    pub device_type: String,
    pub code: String,
    pub expires_at: i64,
    pub created_at: i64,
}

impl StateDb {
    /// Open or create the state database.
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        
        // Create tables if they don't exist
        conn.execute_batch(
            "
            -- Device registry: current state of all known devices
            CREATE TABLE IF NOT EXISTS devices (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                device_type TEXT NOT NULL,
                status TEXT DEFAULT 'offline',
                latitude REAL DEFAULT 0,
                longitude REAL DEFAULT 0,
                altitude REAL DEFAULT 0,
                heading REAL DEFAULT 0,
                speed REAL DEFAULT 0,
                battery REAL DEFAULT 100,
                last_seen INTEGER DEFAULT 0,
                token TEXT,
                paired_at INTEGER DEFAULT 0
            );
            
            -- Pairing requests: pending 6-digit code confirmations
            -- Requests expire after 5 minutes (300 seconds)
            CREATE TABLE IF NOT EXISTS pairing_requests (
                device_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                device_type TEXT NOT NULL,
                code TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );
            
            -- Command history
            CREATE TABLE IF NOT EXISTS commands (
                id TEXT PRIMARY KEY,
                device_id TEXT NOT NULL,
                command_type TEXT NOT NULL,
                payload TEXT DEFAULT '{}',
                status TEXT DEFAULT 'pending',
                created_at INTEGER DEFAULT 0,
                FOREIGN KEY (device_id) REFERENCES devices(id)
            );
            
            -- Indexes for fast lookups
            CREATE INDEX IF NOT EXISTS idx_devices_status ON devices(status);
            CREATE INDEX IF NOT EXISTS idx_devices_token ON devices(token);
            CREATE INDEX IF NOT EXISTS idx_commands_device ON commands(device_id);
            CREATE INDEX IF NOT EXISTS idx_pairing_code ON pairing_requests(code);
            CREATE INDEX IF NOT EXISTS idx_pairing_expires ON pairing_requests(expires_at);
            "
        ).map_err(|e| e.to_string())?;
        
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
    
    // ========================================================================
    // PAIRING
    // ========================================================================
    
    /// Create a new pairing request with a 6-character alphanumeric code.
    /// Returns the generated code.
    pub fn create_pairing_request(&self, device_id: &str, name: &str, device_type: &str) -> Result<String, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_unix();
        let expires_at = now + 300; // 5 minutes
        
        // Generate 6-character alphanumeric code
        let code = generate_code();
        
        // Delete any existing request for this device
        conn.execute(
            "DELETE FROM pairing_requests WHERE device_id = ?1",
            params![device_id],
        ).map_err(|e| e.to_string())?;
        
        // Insert new request
        conn.execute(
            "INSERT INTO pairing_requests (device_id, name, device_type, code, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![device_id, name, device_type, code, now, expires_at],
        ).map_err(|e| e.to_string())?;
        
        Ok(code)
    }
    
    /// Validate a pairing code and create the device with a token.
    /// Returns the auth token on success.
    pub fn confirm_pairing(&self, device_id: &str, code: &str) -> Result<String, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_unix();
        
        // Find the pairing request
        let request: Option<(String, String, String)> = conn.query_row(
            "SELECT name, device_type, code FROM pairing_requests 
             WHERE device_id = ?1 AND code = ?2 AND expires_at > ?3",
            params![device_id, code, now],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok();
        
        match request {
            Some((name, device_type, _)) => {
                // Generate auth token
                let token = generate_token();
                
                // Create or update device with token
                conn.execute(
                    "INSERT INTO devices (id, name, device_type, status, token, paired_at, last_seen)
                     VALUES (?1, ?2, ?3, 'offline', ?4, ?5, ?5)
                     ON CONFLICT(id) DO UPDATE SET
                        name = ?2,
                        device_type = ?3,
                        token = ?4,
                        paired_at = ?5",
                    params![device_id, name, device_type, token, now],
                ).map_err(|e| e.to_string())?;
                
                // Delete the pairing request
                conn.execute(
                    "DELETE FROM pairing_requests WHERE device_id = ?1",
                    params![device_id],
                ).map_err(|e| e.to_string())?;
                
                Ok(token)
            }
            None => Err("Invalid or expired code".to_string()),
        }
    }
    
    /// Get all pending pairing requests (not expired).
    pub fn get_pending_pairing_requests(&self) -> Result<Vec<PairingRequest>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_unix();
        
        let mut stmt = conn.prepare(
            "SELECT device_id, name, device_type, code, expires_at, created_at 
             FROM pairing_requests WHERE expires_at > ?1 ORDER BY created_at DESC"
        ).map_err(|e| e.to_string())?;
        
        let requests = stmt.query_map(params![now], |row| {
            Ok(PairingRequest {
                device_id: row.get(0)?,
                name: row.get(1)?,
                device_type: row.get(2)?,
                code: row.get(3)?,
                expires_at: row.get(4)?,
                created_at: row.get(5)?,
            })
        }).map_err(|e| e.to_string())?;
        
        requests.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
    
    /// Delete a pairing request (dismiss/reject).
    pub fn delete_pairing_request(&self, device_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        conn.execute(
            "DELETE FROM pairing_requests WHERE device_id = ?1",
            params![device_id],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    /// Clean up expired pairing requests.
    pub fn cleanup_expired_requests(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_unix();
        
        let deleted = conn.execute(
            "DELETE FROM pairing_requests WHERE expires_at <= ?1",
            params![now],
        ).map_err(|e| e.to_string())?;
        
        Ok(deleted)
    }
    
    // ========================================================================
    // TOKEN VALIDATION
    // ========================================================================
    
    /// Validate a device token. Returns device_id if valid.
    pub fn validate_token(&self, token: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        let device_id: Option<String> = conn.query_row(
            "SELECT id FROM devices WHERE token = ?1",
            params![token],
            |row| row.get(0),
        ).ok();
        
        Ok(device_id)
    }
    
    /// Revoke a device (delete token, effectively un-pairing).
    pub fn revoke_device(&self, device_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        conn.execute(
            "UPDATE devices SET token = NULL, status = 'revoked' WHERE id = ?1",
            params![device_id],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    /// Delete a device entirely.
    pub fn delete_device(&self, device_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        conn.execute(
            "DELETE FROM devices WHERE id = ?1",
            params![device_id],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    // ========================================================================
    // DEVICE MANAGEMENT
    // ========================================================================
    
    /// Register or update a device.
    pub fn upsert_device(&self, device: &DeviceInfo) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        conn.execute(
            "INSERT INTO devices (id, name, device_type, status, latitude, longitude, altitude, heading, speed, battery, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                name = ?2,
                device_type = ?3,
                status = ?4,
                latitude = ?5,
                longitude = ?6,
                altitude = ?7,
                heading = ?8,
                speed = ?9,
                battery = ?10,
                last_seen = ?11",
            params![
                device.id,
                device.name,
                device.device_type,
                device.status,
                device.latitude,
                device.longitude,
                device.altitude,
                device.heading,
                device.speed,
                device.battery,
                device.last_seen,
            ],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    /// Update device telemetry (position, battery, etc).
    pub fn update_telemetry(&self, device_id: &str, lat: f64, lon: f64, alt: f64, heading: f64, speed: f64, battery: f64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_unix();
        
        conn.execute(
            "UPDATE devices SET 
                latitude = ?1, longitude = ?2, altitude = ?3,
                heading = ?4, speed = ?5, battery = ?6,
                status = 'online', last_seen = ?7
             WHERE id = ?8",
            params![lat, lon, alt, heading, speed, battery, now, device_id],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    /// Set device status (online, offline, etc).
    pub fn set_status(&self, device_id: &str, status: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_unix();
        
        conn.execute(
            "UPDATE devices SET status = ?1, last_seen = ?2 WHERE id = ?3",
            params![status, now, device_id],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    /// Get all devices (only paired ones with tokens).
    pub fn get_all_devices(&self) -> Result<Vec<DeviceInfo>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        let mut stmt = conn.prepare(
            "SELECT id, name, device_type, status, latitude, longitude, altitude, heading, speed, battery, last_seen 
             FROM devices WHERE token IS NOT NULL ORDER BY last_seen DESC"
        ).map_err(|e| e.to_string())?;
        
        let devices = stmt.query_map([], |row| {
            Ok(DeviceInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                device_type: row.get(2)?,
                status: row.get(3)?,
                latitude: row.get(4)?,
                longitude: row.get(5)?,
                altitude: row.get(6)?,
                heading: row.get(7)?,
                speed: row.get(8)?,
                battery: row.get(9)?,
                last_seen: row.get(10)?,
            })
        }).map_err(|e| e.to_string())?;
        
        devices.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }
    
    /// Get a single device by ID.
    pub fn get_device(&self, device_id: &str) -> Result<Option<DeviceInfo>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        let device = conn.query_row(
            "SELECT id, name, device_type, status, latitude, longitude, altitude, heading, speed, battery, last_seen 
             FROM devices WHERE id = ?1",
            params![device_id],
            |row| {
                Ok(DeviceInfo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    device_type: row.get(2)?,
                    status: row.get(3)?,
                    latitude: row.get(4)?,
                    longitude: row.get(5)?,
                    altitude: row.get(6)?,
                    heading: row.get(7)?,
                    speed: row.get(8)?,
                    battery: row.get(9)?,
                    last_seen: row.get(10)?,
                })
            },
        ).ok();
        
        Ok(device)
    }
    
    // ========================================================================
    // COMMANDS
    // ========================================================================
    
    /// Save a command.
    pub fn save_command(&self, id: &str, device_id: &str, command_type: &str, payload: &str, status: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = now_unix();
        
        conn.execute(
            "INSERT INTO commands (id, device_id, command_type, payload, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, device_id, command_type, payload, status, now],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    /// Update command status.
    pub fn update_command_status(&self, id: &str, status: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        conn.execute(
            "UPDATE commands SET status = ?1 WHERE id = ?2",
            params![status, id],
        ).map_err(|e| e.to_string())?;
        
        Ok(())
    }
    
    /// Clone for thread sharing.
    #[allow(dead_code)]
    pub fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
        }
    }
}

// ============================================================================
// UTILITIES
// ============================================================================

/// Get current unix timestamp.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Generate a 6-character alphanumeric code (A-Z, 0-9).
fn generate_code() -> String {
    let chars = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // Removed confusable chars: I, O, 0, 1
    let mut code = String::with_capacity(6);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    
    for i in 0..6 {
        let idx = ((t >> (i * 8)) ^ (t >> (i * 4 + 3))) as usize % chars.len();
        code.push(chars[idx] as char);
    }
    
    code
}

/// Generate a 64-character hex token.
fn generate_token() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    
    // Mix time with some shifting to create pseudo-random token
    let mut token = String::with_capacity(64);
    for i in 0..8 {
        let val = (t >> (i * 16)) ^ (t.wrapping_mul(0x5851F42D4C957F2D_u128) >> (i * 8));
        token.push_str(&format!("{:016x}", val as u64));
    }
    
    token.truncate(64);
    token
}
