//! # Telemetry Storage
//! 
//! High-throughput, file-based telemetry storage.
//! 
//! WHY FILES (not database):
//! - Maximum write throughput: just append bytes
//! - No contention: each device has its own file
//! - AI training ready: files are what ML pipelines expect
//! - Embarrassingly parallel: shard across machines by copying files
//! - 1000-year-proof: bytes on disk, no schema versioning
//! 
//! STRUCTURE:
//! data/telemetry/YYYY/MM/DD/{device-id}.jsonl
//! 
//! Each line is a JSON object with timestamp and telemetry data.
//! JSONL (JSON Lines) is simple, streamable, and universally readable.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

/// A single telemetry record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub timestamp: i64,
    pub device_id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub heading: f64,
    pub speed: f64,
    pub battery: f64,
    #[serde(default)]
    pub sensors: serde_json::Value,
}

/// Telemetry writer that manages file handles per device.
pub struct TelemetryWriter {
    base_path: PathBuf,
    writers: Arc<Mutex<HashMap<String, BufWriter<File>>>>,
    last_flush: Arc<Mutex<i64>>,
}

impl TelemetryWriter {
    /// Create a new telemetry writer.
    pub fn new(base_path: &str) -> Self {
        Self {
            base_path: PathBuf::from(base_path),
            writers: Arc::new(Mutex::new(HashMap::new())),
            last_flush: Arc::new(Mutex::new(0)),
        }
    }
    
    /// Write a telemetry record.
    /// Creates directory structure and file as needed.
    pub fn write(&self, record: &TelemetryRecord) -> Result<(), String> {
        let now = now_unix();
        let (year, month, day) = date_parts(now);
        
        // Build path: data/telemetry/YYYY/MM/DD/{device-id}.jsonl
        let dir = self.base_path
            .join(format!("{:04}", year))
            .join(format!("{:02}", month))
            .join(format!("{:02}", day));
        
        let file_path = dir.join(format!("{}.jsonl", record.device_id));
        
        // Get or create writer
        let mut writers = self.writers.lock().map_err(|e| e.to_string())?;
        
        let writer = if let Some(w) = writers.get_mut(&record.device_id) {
            w
        } else {
            // Create directory if needed
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            
            // Open file for append
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)
                .map_err(|e| e.to_string())?;
            
            writers.insert(record.device_id.clone(), BufWriter::new(file));
            writers.get_mut(&record.device_id).unwrap()
        };
        
        // Write JSON line
        let json = serde_json::to_string(record).map_err(|e| e.to_string())?;
        writeln!(writer, "{}", json).map_err(|e| e.to_string())?;
        
        // Periodic flush (every 5 seconds)
        let mut last_flush = self.last_flush.lock().map_err(|e| e.to_string())?;
        if now - *last_flush > 5 {
            for w in writers.values_mut() {
                let _ = w.flush();
            }
            *last_flush = now;
        }
        
        Ok(())
    }
    
    /// Flush all writers.
    pub fn flush(&self) -> Result<(), String> {
        let mut writers = self.writers.lock().map_err(|e| e.to_string())?;
        for w in writers.values_mut() {
            w.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    
    /// Clone for thread sharing.
    pub fn clone(&self) -> Self {
        Self {
            base_path: self.base_path.clone(),
            writers: Arc::clone(&self.writers),
            last_flush: Arc::clone(&self.last_flush),
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

/// Extract year, month, day from unix timestamp.
fn date_parts(timestamp: i64) -> (i32, u32, u32) {
    // Simple date calculation (not accounting for leap seconds, etc.)
    // Good enough for directory naming.
    let days_since_epoch = timestamp / 86400;
    
    // Approximate calculation
    let mut year = 1970;
    let mut remaining_days = days_since_epoch;
    
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    
    let days_in_months: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    
    let mut month = 1;
    for days in days_in_months.iter() {
        if remaining_days < *days {
            break;
        }
        remaining_days -= days;
        month += 1;
    }
    
    let day = remaining_days + 1;
    
    (year, month as u32, day as u32)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
