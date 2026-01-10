//! # State Management
//! 
//! SQLite-based state storage for device registry and commands.
//! 
//! WHY SQLITE:
//! - It's a file. No database server. No connection management.
//! - Embedded in binary via rusqlite's "bundled" feature.
//! - 20+ years of backwards compatibility.
//! - Any tool can inspect the database file.
//! 
//! This handles:
//! - Device registration and current state
//! - Command queue and history
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
                last_seen INTEGER DEFAULT 0
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
            CREATE INDEX IF NOT EXISTS idx_commands_device ON commands(device_id);
            "
        ).map_err(|e| e.to_string())?;
        
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
    
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
    
    /// Get all devices.
    pub fn get_all_devices(&self) -> Result<Vec<DeviceInfo>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        
        let mut stmt = conn.prepare(
            "SELECT id, name, device_type, status, latitude, longitude, altitude, heading, speed, battery, last_seen 
             FROM devices ORDER BY last_seen DESC"
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

/// Get current unix timestamp.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
