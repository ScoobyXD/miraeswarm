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
//!                    │ │ State │ │ ← SQLite (device registry, pairing)
//!                    │ └───────┘ │
//!                    │ ┌───────┐ │
//!                    │ │ Telem │ │ ← Files (time-series data)
//!                    │ └───────┘ │
//!                    └───────────┘
//! ```
//!
//! ## Device Connection Flow
//! 
//! 1. Device POSTs to /api/pair/request → Gets "pending" status
//! 2. Server generates 6-digit code, broadcasts to GlobalUI
//! 3. User tells device operator the code
//! 4. Device POSTs to /api/pair/confirm with code → Gets auth token
//! 5. Device connects via WebSocket with token → Fully connected

mod protocol;
mod websocket;
mod state;
mod telemetry;
mod http;

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use protocol::{Envelope, DeviceInfo, TelemetryMessage, RegisterMessage, SendCommand};
use websocket::{WebSocket, State as WsState};
use state::StateDb;
use telemetry::{TelemetryWriter, TelemetryRecord};

// ============================================================================
// CONFIGURATION
// ============================================================================

const PORT: u16 = 3000;
const PUBLIC_DIR: &str = "public";
const DATA_DIR: &str = "data";
const DB_FILE: &str = "data/state.db";
const PAIRING_BROADCAST_INTERVAL_MS: u64 = 1000;

// ============================================================================
// SERVER STATE
// ============================================================================

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

struct Server {
    clients: HashMap<usize, Client>,
    next_id: usize,
    db: StateDb,
    telemetry: TelemetryWriter,
}

impl Server {
    fn new() -> Result<Self, String> {
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
    
    /// Broadcast pending pairing requests to all UIs
    fn broadcast_pairing_requests(&mut self) {
        if let Ok(requests) = self.db.get_pending_pairing_requests() {
            if !requests.is_empty() {
                let json: Vec<serde_json::Value> = requests.iter().map(|r| {
                    serde_json::json!({
                        "device_id": r.device_id,
                        "name": r.name,
                        "device_type": r.device_type,
                        "code": r.code,
                        "expires_at": r.expires_at
                    })
                }).collect();
                
                self.broadcast_to_uis(&Envelope::new("pairing:requests", &serde_json::json!({
                    "requests": json
                })));
            }
        }
    }
}

// ============================================================================
// MESSAGE HANDLING
// ============================================================================

fn handle_message(server: &mut Server, client_id: usize, msg: &str) {
    let envelope: Envelope = match serde_json::from_str(msg) {
        Ok(e) => e,
        Err(_) => return,
    };
    
    match envelope.msg_type.as_str() {
        // Device registration (with token auth)
        "register" => {
            if let Ok(reg) = serde_json::from_value::<RegisterMessage>(envelope.data) {
                // Validate token
                let token = reg.token.as_deref().unwrap_or("");
                
                if !token.is_empty() {
                    // Check if token is valid
                    match server.db.validate_token(token) {
                        Ok(Some(stored_device_id)) => {
                            // Token valid - use the device_id from token if different
                            let device_id = if reg.device_id.is_empty() { 
                                stored_device_id.clone() 
                            } else { 
                                reg.device_id.clone() 
                            };
                            
                            let now = now_unix();
                            let device = DeviceInfo {
                                id: device_id.clone(),
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
                            
                            let _ = server.db.upsert_device(&device);
                            
                            if let Some(client) = server.clients.get_mut(&client_id) {
                                client.client_type = ClientType::Device;
                                client.device_id = Some(device_id.clone());
                                let _ = client.ws.send(&Envelope::new("registered", &serde_json::json!({
                                    "status": "ok",
                                    "device": device
                                })).to_json());
                            }
                            
                            server.broadcast_to_uis(&Envelope::new("device:online", &device));
                            println!("✓ Device registered: {} ({})", reg.name, reg.device_type);
                        }
                        Ok(None) => {
                            // Invalid token
                            if let Some(client) = server.clients.get_mut(&client_id) {
                                let _ = client.ws.send(&Envelope::new("error", &serde_json::json!({
                                    "code": "invalid_token",
                                    "message": "Invalid or expired token. Please re-pair the device."
                                })).to_json());
                            }
                            println!("✗ Invalid token from device: {}", reg.device_id);
                        }
                        Err(e) => {
                            if let Some(client) = server.clients.get_mut(&client_id) {
                                let _ = client.ws.send(&Envelope::new("error", &serde_json::json!({
                                    "code": "db_error",
                                    "message": e
                                })).to_json());
                            }
                        }
                    }
                } else {
                    // No token provided - reject
                    if let Some(client) = server.clients.get_mut(&client_id) {
                        let _ = client.ws.send(&Envelope::new("error", &serde_json::json!({
                            "code": "no_token",
                            "message": "Authentication required. Use /api/pair/request to get a token."
                        })).to_json());
                    }
                    println!("✗ Device tried to register without token: {}", reg.device_id);
                }
            }
        }
        
        // Device telemetry
        "telemetry" => {
            if let Ok(telem) = serde_json::from_value::<TelemetryMessage>(envelope.data.clone()) {
                let device_id = server.clients.get(&client_id)
                    .and_then(|c| c.device_id.clone());
                
                if let Some(device_id) = device_id {
                    let _ = server.db.update_telemetry(
                        &device_id,
                        telem.latitude,
                        telem.longitude,
                        telem.altitude,
                        telem.heading,
                        telem.speed,
                        telem.battery,
                    );
                    
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
                    
                    server.broadcast_to_uis(&Envelope::new("device:update", &device_update));
                }
            }
        }
        
        // UI requesting device list
        "getDevices" => {
            if let Some(client) = server.clients.get_mut(&client_id) {
                client.client_type = ClientType::Ui;
                
                if let Ok(devices) = server.db.get_all_devices() {
                    let _ = client.ws.send(&Envelope::new("devices:list", &devices).to_json());
                }
                
                // Also send pending pairing requests
                if let Ok(requests) = server.db.get_pending_pairing_requests() {
                    let json: Vec<serde_json::Value> = requests.iter().map(|r| {
                        serde_json::json!({
                            "device_id": r.device_id,
                            "name": r.name,
                            "device_type": r.device_type,
                            "code": r.code,
                            "expires_at": r.expires_at
                        })
                    }).collect();
                    let _ = client.ws.send(&Envelope::new("pairing:requests", &serde_json::json!({
                        "requests": json
                    })).to_json());
                }
            }
            println!("✓ GlobalUI connected");
        }
        
        // UI dismissing a pairing request
        "dismissPairing" => {
            if let Some(device_id) = envelope.data.get("device_id").and_then(|v| v.as_str()) {
                let _ = server.db.delete_pairing_request(device_id);
                println!("✗ Pairing dismissed: {}", device_id);
            }
        }
        
        // UI revoking a device
        "revokeDevice" => {
            if let Some(device_id) = envelope.data.get("device_id").and_then(|v| v.as_str()) {
                let _ = server.db.delete_device(device_id);
                server.broadcast_to_uis(&Envelope::new("device:revoked", &serde_json::json!({
                    "device_id": device_id
                })));
                println!("✗ Device revoked: {}", device_id);
            }
        }
        
        // UI sending command to device
        "sendCommand" => {
            if let Ok(cmd) = serde_json::from_value::<SendCommand>(envelope.data) {
                let command_id = generate_id();
                let payload_str = cmd.payload.to_string();
                let _ = server.db.save_command(&command_id, &cmd.device_id, &cmd.command_type, &payload_str, "pending");
                
                let sent = server.send_to_device(&cmd.device_id, &Envelope::new("command", &serde_json::json!({
                    "commandId": command_id,
                    "type": cmd.command_type,
                    "payload": cmd.payload,
                })));
                
                let status = if sent { "sent" } else { "failed" };
                let _ = server.db.update_command_status(&command_id, status);
                
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
    
    let server = match Server::new() {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            eprintln!("Failed to initialize server: {}", e);
            return;
        }
    };
    
    // Start pairing broadcast thread
    {
        let server = Arc::clone(&server);
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(PAIRING_BROADCAST_INTERVAL_MS));
                if let Ok(mut server) = server.lock() {
                    server.broadcast_pairing_requests();
                    // Also cleanup expired requests
                    let _ = server.db.cleanup_expired_requests();
                }
            }
        });
    }
    
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
    println!("\n  API Endpoints:");
    println!("    POST /api/pair/request  - Device requests to join");
    println!("    POST /api/pair/confirm  - Device confirms with code");
    println!("    GET  /api/devices       - List paired devices");
    println!("\n============================================\n");
    
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

fn handle_connection(mut stream: TcpStream, server: Arc<Mutex<Server>>) {
    let request = match http::read_request(&mut stream) {
        Ok(r) => r,
        Err(_) => return,
    };
    
    if http::handle_request(&mut stream, &request, PUBLIC_DIR) {
        return;
    }
    
    let ws = match WebSocket::accept(stream, &request) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("WebSocket handshake failed: {}", e);
            return;
        }
    };
    
    let client_id = {
        let mut server = server.lock().unwrap();
        server.add_client(ws.try_clone().unwrap())
    };
    
    let mut ws = ws;
    loop {
        match ws.read() {
            Ok(Some(msg)) => {
                let mut server = server.lock().unwrap();
                handle_message(&mut server, client_id, &msg);
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
        
        if ws.state != WsState::Open {
            break;
        }
    }
    
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
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ((t >> 16) ^ t) as u16
}
