//! # GlobalRTS Server
//! 
//! Command center for robot fleets.
//! 
//! Single binary. No runtime dependencies. 1000-year-proof.
//! 
//! ## Architecture
//! 
//! ```text
//! Browser (GlobalUI)                    Devices (Robots/Phones)
//!        │                                      │
//!        └──────────── WebSocket ───────────────┘
//!                          │
//!                    ┌─────┴─────┐
//!                    │  Server   │
//!                    │           │
//!                    │ ┌───────┐ │
//!                    │ │ State │ │ ← SQLite (device registry)
//!                    │ └───────┘ │
//!                    │ ┌───────┐ │
//!                    │ │ Telem │ │ ← Files (time-series data)
//!                    │ └───────┘ │
//!                    └───────────┘
//! ```

mod protocol;
mod websocket;
mod state;
mod telemetry;
mod http;

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use protocol::{Envelope, DeviceInfo, TelemetryMessage, RegisterMessage, SendCommand};
use websocket::{WebSocket, State as WsState};
use state::StateDb;
use telemetry::{TelemetryWriter, TelemetryRecord};

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Server configuration. Edit these constants directly.
/// No config files. No environment variables. Just change the code.
const PORT: u16 = 3000;
const PUBLIC_DIR: &str = "public";
const DATA_DIR: &str = "data";
const DB_FILE: &str = "data/state.db";

// ============================================================================
// SERVER STATE
// ============================================================================

/// Connected client info.
struct Client {
    ws: WebSocket,
    client_type: ClientType,
    device_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum ClientType {
    Unknown,
    Device,
    Ui,
}

/// Shared server state.
struct Server {
    clients: HashMap<usize, Client>,
    next_id: usize,
    db: StateDb,
    telemetry: TelemetryWriter,
}

impl Server {
    fn new() -> Result<Self, String> {
        // Ensure data directory exists
        std::fs::create_dir_all(DATA_DIR).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(format!("{}/telemetry", DATA_DIR)).map_err(|e| e.to_string())?;
        
        Ok(Self {
            clients: HashMap::new(),
            next_id: 0,
            db: StateDb::open(DB_FILE)?,
            telemetry: TelemetryWriter::new(&format!("{}/telemetry", DATA_DIR)),
        })
    }
    
    fn add_client(&mut self, ws: WebSocket) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.clients.insert(id, Client {
            ws,
            client_type: ClientType::Unknown,
            device_id: None,
        });
        id
    }
    
    fn remove_client(&mut self, id: usize) {
        if let Some(client) = self.clients.remove(&id) {
            if let Some(device_id) = &client.device_id {
                let _ = self.db.set_status(device_id, "offline");
                self.broadcast_to_uis(&Envelope::new("device:offline", &serde_json::json!({
                    "deviceId": device_id
                })));
                println!("✗ Device disconnected: {}", device_id);
            }
        }
    }
    
    fn broadcast_to_uis(&mut self, envelope: &Envelope) {
        let json = envelope.to_json();
        for client in self.clients.values_mut() {
            if client.client_type == ClientType::Ui {
                let _ = client.ws.send(&json);
            }
        }
    }
    
    fn send_to_device(&mut self, device_id: &str, envelope: &Envelope) -> bool {
        let json = envelope.to_json();
        for client in self.clients.values_mut() {
            if client.device_id.as_deref() == Some(device_id) {
                return client.ws.send(&json).is_ok();
            }
        }
        false
    }
}

// ============================================================================
// MESSAGE HANDLING
// ============================================================================

fn handle_message(server: &mut Server, client_id: usize, msg: &str) {
    // Parse envelope
    let envelope: Envelope = match serde_json::from_str(msg) {
        Ok(e) => e,
        Err(_) => return,
    };
    
    match envelope.msg_type.as_str() {
        // Device registration
        "register" => {
            if let Ok(reg) = serde_json::from_value::<RegisterMessage>(envelope.data) {
                let now = now_unix();
                
                let device = DeviceInfo {
                    id: reg.device_id.clone(),
                    name: reg.name.clone(),
                    device_type: reg.device_type.clone(),
                    status: "online".to_string(),
                    latitude: reg.latitude,
                    longitude: reg.longitude,
                    altitude: reg.altitude,
                    heading: 0.0,
                    speed: 0.0,
                    battery: 100.0,
                    last_seen: now,
                };
                
                // Save to database
                let _ = server.db.upsert_device(&device);
                
                // Update client info
                if let Some(client) = server.clients.get_mut(&client_id) {
                    client.client_type = ClientType::Device;
                    client.device_id = Some(reg.device_id.clone());
                    
                    // Confirm registration
                    let _ = client.ws.send(&Envelope::new("registered", &serde_json::json!({
                        "device": device
                    })).to_json());
                }
                
                // Notify UIs
                server.broadcast_to_uis(&Envelope::new("device:online", &device));
                
                println!("✓ Device registered: {} ({})", reg.name, reg.device_type);
            }
        }
        
        // Device telemetry
        "telemetry" => {
            if let Ok(telem) = serde_json::from_value::<TelemetryMessage>(envelope.data.clone()) {
                // Get device ID for this client
                let device_id = server.clients.get(&client_id)
                    .and_then(|c| c.device_id.clone());
                
                if let Some(device_id) = device_id {
                    // Update state DB
                    let _ = server.db.update_telemetry(
                        &device_id,
                        telem.latitude,
                        telem.longitude,
                        telem.altitude,
                        telem.heading,
                        telem.speed,
                        telem.battery,
                    );
                    
                    // Write to telemetry files
                    let record = TelemetryRecord {
                        timestamp: now_unix(),
                        device_id: device_id.clone(),
                        latitude: telem.latitude,
                        longitude: telem.longitude,
                        altitude: telem.altitude,
                        heading: telem.heading,
                        speed: telem.speed,
                        battery: telem.battery,
                        sensors: telem.sensors.clone(),
                    };
                    let _ = server.telemetry.write(&record);
                    
                    // Build device info for broadcast
                    let device_update = serde_json::json!({
                        "id": device_id,
                        "latitude": telem.latitude,
                        "longitude": telem.longitude,
                        "altitude": telem.altitude,
                        "heading": telem.heading,
                        "speed": telem.speed,
                        "battery": telem.battery,
                        "status": "online",
                    });
                    
                    // Broadcast to UIs
                    server.broadcast_to_uis(&Envelope::new("device:update", &device_update));
                }
            }
        }
        
        // UI requesting device list
        "getDevices" => {
            // Mark as UI client
            if let Some(client) = server.clients.get_mut(&client_id) {
                client.client_type = ClientType::Ui;
                
                // Send device list
                if let Ok(devices) = server.db.get_all_devices() {
                    let _ = client.ws.send(&Envelope::new("devices:list", &devices).to_json());
                }
            }
            println!("✓ GlobalUI connected");
        }
        
        // UI sending command to device
        "sendCommand" => {
            if let Ok(cmd) = serde_json::from_value::<SendCommand>(envelope.data) {
                let command_id = generate_id();
                
                // Save command to DB
                let payload_str = cmd.payload.to_string();
                let _ = server.db.save_command(&command_id, &cmd.device_id, &cmd.command_type, &payload_str, "pending");
                
                // Send to device
                let sent = server.send_to_device(&cmd.device_id, &Envelope::new("command", &serde_json::json!({
                    "commandId": command_id,
                    "type": cmd.command_type,
                    "payload": cmd.payload,
                })));
                
                let status = if sent { "sent" } else { "failed" };
                let _ = server.db.update_command_status(&command_id, status);
                
                // Confirm to UI
                if let Some(client) = server.clients.get_mut(&client_id) {
                    let _ = client.ws.send(&Envelope::new("command:sent", &serde_json::json!({
                        "commandId": command_id,
                        "deviceId": cmd.device_id,
                        "status": status,
                    })).to_json());
                }
                
                println!("→ Command: {} -> {} ({})", cmd.command_type, cmd.device_id, status);
            }
        }
        
        // Device acknowledging command
        "command:ack" | "command:complete" => {
            if let Some(command_id) = envelope.data.get("commandId").and_then(|v| v.as_str()) {
                let status = envelope.data.get("status").and_then(|v| v.as_str()).unwrap_or("acknowledged");
                let _ = server.db.update_command_status(command_id, status);
                
                // Forward to UIs
                server.broadcast_to_uis(&envelope);
            }
        }
        
        _ => {}
    }
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    println!("\n============================================");
    println!("  GLOBALRTS - COMMAND CENTER");
    println!("============================================");
    println!("  Observable • Reprogrammable • 1000-Year-Proof");
    println!("============================================\n");
    
    // Initialize server state
    let server = match Server::new() {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            eprintln!("Failed to initialize server: {}", e);
            return;
        }
    };
    
    // Start TCP listener
    let addr = format!("0.0.0.0:{}", PORT);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {}: {}", addr, e);
            return;
        }
    };
    
    println!("✓ Server running on http://localhost:{}", PORT);
    println!("\n  GlobalUI: http://localhost:{}/globalui.html", PORT);
    println!("  WebSocket: ws://localhost:{}", PORT);
    println!("\n============================================\n");
    
    // Accept connections
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let server = Arc::clone(&server);
                thread::spawn(move || {
                    handle_connection(stream, server);
                });
            }
            Err(e) => eprintln!("Connection failed: {}", e),
        }
    }
}

/// Handle a single TCP connection.
fn handle_connection(mut stream: TcpStream, server: Arc<Mutex<Server>>) {
    // Read HTTP request
    let request = match http::read_request(&mut stream) {
        Ok(r) => r,
        Err(_) => return,
    };
    
    // Check if this is HTTP or WebSocket
    if http::handle_request(&mut stream, &request, PUBLIC_DIR) {
        return; // Was HTTP request, done
    }
    
    // WebSocket upgrade
    let ws = match WebSocket::accept(stream, &request) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("WebSocket handshake failed: {}", e);
            return;
        }
    };
    
    // Register client
    let client_id = {
        let mut server = server.lock().unwrap();
        server.add_client(ws.try_clone().unwrap())
    };
    
    // Message loop
    let mut ws = ws;
    loop {
        // Check for messages
        match ws.read() {
            Ok(Some(msg)) => {
                let mut server = server.lock().unwrap();
                handle_message(&mut server, client_id, &msg);
            }
            Ok(None) => {
                // No message available, sleep briefly
                thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => break,
        }
        
        // Check if connection closed
        if ws.state != WsState::Open {
            break;
        }
    }
    
    // Cleanup
    let mut server = server.lock().unwrap();
    server.remove_client(client_id);
}

// ============================================================================
// UTILITIES
// ============================================================================

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn generate_id() -> String {
    format!("{:x}-{:04x}", now_unix(), rand_u16())
}

fn rand_u16() -> u16 {
    // Simple PRNG using time - good enough for IDs
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ((t >> 16) ^ t) as u16
}
