//! # Device Simulator
//! 
//! Simulates robots, phones, drones connecting to the command center.
//! 
//! USAGE:
//!   cargo run --bin simulator -- [type] [id] [name]
//!   ./simulator robot robot-01 "Robot Alpha"
//!   ./simulator phone phone-01 "Jonathan's iPhone"
//!   ./simulator drone drone-01 "Aerial Scout"

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::thread;

use base64::Engine;
use serde::{Serialize, Deserialize};

// ============================================================================
// CONFIGURATION
// ============================================================================

const SERVER_HOST: &str = "127.0.0.1";
const SERVER_PORT: u16 = 3000;
const TELEMETRY_INTERVAL_MS: u64 = 1000;

// ============================================================================
// WEBSOCKET CLIENT (minimal implementation)
// ============================================================================

struct WsClient {
    stream: TcpStream,
}

impl WsClient {
    fn connect(host: &str, port: u16) -> Result<Self, String> {
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
        
        // Generate random key
        let key = base64::engine::general_purpose::STANDARD.encode(rand_bytes());
        
        // Send upgrade request
        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: {}:{}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n",
            host, port, key
        );
        stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
        
        // Read response
        let mut buf = [0u8; 1024];
        stream.read(&mut buf).map_err(|e| e.to_string())?;
        
        let response = String::from_utf8_lossy(&buf);
        if !response.contains("101") {
            return Err("WebSocket upgrade failed".to_string());
        }
        
        stream.set_nonblocking(true).map_err(|e| e.to_string())?;
        
        Ok(Self { stream })
    }
    
    fn send(&mut self, msg: &str) -> Result<(), String> {
        let payload = msg.as_bytes();
        let len = payload.len();
        
        let mut frame = Vec::new();
        
        // Header: FIN + TEXT opcode
        frame.push(0x81);
        
        // Length + mask bit
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else {
            frame.push(0x80 | 126);
            frame.push((len >> 8) as u8);
            frame.push(len as u8);
        }
        
        // Masking key
        let mask = rand_bytes();
        frame.extend_from_slice(&mask);
        
        // Masked payload
        for (i, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[i % 4]);
        }
        
        self.stream.write_all(&frame).map_err(|e| e.to_string())
    }
    
    fn recv(&mut self) -> Option<String> {
        let mut header = [0u8; 2];
        match self.stream.read_exact(&mut header) {
            Ok(_) => {}
            Err(_) => return None,
        }
        
        let len = (header[1] & 0x7F) as usize;
        let mut payload = vec![0u8; len];
        
        match self.stream.read_exact(&mut payload) {
            Ok(_) => Some(String::from_utf8_lossy(&payload).to_string()),
            Err(_) => None,
        }
    }
}

fn rand_bytes() -> [u8; 4] {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    [
        (t >> 24) as u8,
        (t >> 16) as u8,
        (t >> 8) as u8,
        t as u8,
    ]
}

// ============================================================================
// PROTOCOL
// ============================================================================

#[derive(Serialize)]
struct Envelope<T> {
    #[serde(rename = "type")]
    msg_type: String,
    data: T,
}

#[derive(Serialize)]
struct RegisterData {
    device_id: String,
    device_type: String,
    name: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Serialize)]
struct TelemetryData {
    latitude: f64,
    longitude: f64,
    altitude: f64,
    heading: f64,
    speed: f64,
    battery: f64,
}

#[derive(Deserialize)]
struct CommandEnvelope {
    #[serde(rename = "type")]
    msg_type: String,
    data: serde_json::Value,
}

// ============================================================================
// DEVICE STATE
// ============================================================================

struct DeviceState {
    lat: f64,
    lon: f64,
    heading: f64,
    speed: f64,
    battery: f64,
    target: Option<(f64, f64)>,
    status: String,
}

impl DeviceState {
    fn new() -> Self {
        // Start in Downtown LA with random offset
        Self {
            lat: 34.0522 + (rand_f64() - 0.5) * 0.01,
            lon: -118.2437 + (rand_f64() - 0.5) * 0.01,
            heading: rand_f64() * 360.0,
            speed: 0.0,
            battery: 85.0 + rand_f64() * 15.0,
            target: None,
            status: "idle".to_string(),
        }
    }
    
    fn update(&mut self) {
        // Move towards target if set
        if let Some((target_lat, target_lon)) = self.target {
            let dlat = target_lat - self.lat;
            let dlon = target_lon - self.lon;
            let dist = (dlat * dlat + dlon * dlon).sqrt();
            
            if dist < 0.0001 {
                // Arrived
                self.lat = target_lat;
                self.lon = target_lon;
                self.speed = 0.0;
                self.target = None;
                self.status = "idle".to_string();
                println!("   ✓ Arrived at destination");
            } else {
                // Move
                let step = 0.0002; // ~22m per tick
                self.lat += (dlat / dist) * step;
                self.lon += (dlon / dist) * step;
                self.heading = dlon.atan2(dlat).to_degrees();
                self.speed = step * 111000.0; // Approximate m/s
            }
        }
        
        // Drain battery
        self.battery = (self.battery - 0.001).max(0.0);
    }
}

fn rand_f64() -> f64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ((t % 1000000) as f64) / 1000000.0
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    let args: Vec<String> = std::env::args().collect();
    
    let device_type = args.get(1).map(|s| s.as_str()).unwrap_or("robot");
    let device_id = args.get(2).cloned().unwrap_or_else(|| {
        format!("{}-{:x}", device_type, SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0))
    });
    let name = args.get(3).cloned().unwrap_or_else(|| {
        format!("Simulated {}", device_type)
    });
    
    println!("\n========================================");
    println!("  DEVICE SIMULATOR");
    println!("========================================");
    println!("  Type: {}", device_type);
    println!("  ID:   {}", device_id);
    println!("  Name: {}", name);
    println!("========================================\n");
    
    // Connect
    println!("Connecting to {}:{}...", SERVER_HOST, SERVER_PORT);
    let mut ws = match WsClient::connect(SERVER_HOST, SERVER_PORT) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to connect: {}", e);
            return;
        }
    };
    println!("✓ Connected\n");
    
    // Initialize state
    let mut state = DeviceState::new();
    
    // Register
    let reg = Envelope {
        msg_type: "register".to_string(),
        data: RegisterData {
            device_id: device_id.clone(),
            device_type: device_type.to_string(),
            name: name.clone(),
            latitude: state.lat,
            longitude: state.lon,
        },
    };
    ws.send(&serde_json::to_string(&reg).unwrap()).unwrap();
    println!("✓ Registered as {}\n", name);
    
    // Main loop
    let mut tick = 0u64;
    loop {
        // Check for commands
        if let Some(msg) = ws.recv() {
            if let Ok(env) = serde_json::from_str::<CommandEnvelope>(&msg) {
                if env.msg_type == "command" {
                    handle_command(&mut ws, &mut state, &device_id, &env.data);
                }
            }
        }
        
        // Update state
        state.update();
        
        // Send telemetry
        let telem = Envelope {
            msg_type: "telemetry".to_string(),
            data: TelemetryData {
                latitude: state.lat,
                longitude: state.lon,
                altitude: 0.0,
                heading: state.heading,
                speed: state.speed,
                battery: state.battery,
            },
        };
        let _ = ws.send(&serde_json::to_string(&telem).unwrap());
        
        // Log status
        tick += 1;
        if tick % 10 == 0 {
            println!("📍 {:.6}, {:.6} | 🔋 {:.1}% | {}", 
                state.lat, state.lon, state.battery, state.status);
        }
        
        thread::sleep(Duration::from_millis(TELEMETRY_INTERVAL_MS));
    }
}

fn handle_command(ws: &mut WsClient, state: &mut DeviceState, _device_id: &str, data: &serde_json::Value) {
    let cmd_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let cmd_id = data.get("commandId").and_then(|v| v.as_str()).unwrap_or("");
    let payload = data.get("payload").cloned().unwrap_or_default();
    
    println!("\n📥 Command: {}", cmd_type);
    
    // Acknowledge
    let ack = serde_json::json!({
        "type": "command:ack",
        "data": { "commandId": cmd_id, "status": "received" }
    });
    let _ = ws.send(&ack.to_string());
    
    match cmd_type {
        "navigate" => {
            let lat = payload.get("latitude").and_then(|v| v.as_f64()).unwrap_or(state.lat);
            let lon = payload.get("longitude").and_then(|v| v.as_f64()).unwrap_or(state.lon);
            state.target = Some((lat, lon));
            state.status = "moving".to_string();
            println!("   🚀 Navigating to {:.6}, {:.6}", lat, lon);
        }
        "stop" => {
            state.target = None;
            state.speed = 0.0;
            state.status = "idle".to_string();
            println!("   🛑 Stopped");
            
            let complete = serde_json::json!({
                "type": "command:complete",
                "data": { "commandId": cmd_id, "status": "completed" }
            });
            let _ = ws.send(&complete.to_string());
        }
        "ring" => {
            println!("   🔔 RING RING RING!");
            state.status = "ringing".to_string();
            thread::sleep(Duration::from_secs(2));
            state.status = "idle".to_string();
            
            let complete = serde_json::json!({
                "type": "command:complete",
                "data": { "commandId": cmd_id, "status": "completed" }
            });
            let _ = ws.send(&complete.to_string());
        }
        _ => {
            println!("   ❓ Unknown command");
        }
    }
    println!();
}
