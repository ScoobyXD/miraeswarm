//! # HTTP Server
//! 
//! Minimal HTTP server for static files and API endpoints.
//! 
//! ENDPOINTS:
//! - GET  /                         → GlobalUI (static HTML)
//! - GET  /api/pair/requests        → List pending pairing requests
//! - POST /api/pair/request         → Device requests to join
//! - POST /api/pair/confirm         → Device confirms with 6-digit code
//! - DELETE /api/pair/{id}          → Dismiss/reject pairing request
//! - GET  /api/devices              → List all paired devices
//! - DELETE /api/devices/{id}       → Revoke device
//! - GET  /api/oura/*               → Proxy to Oura Ring API (any path)
//! 
//! WHY FROM SCRATCH:
//! - We need ~400 lines, not a framework
//! - Static file serving + simple REST is trivial
//! - No dependency that can break

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write, BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;

use crate::state::StateDb;

/// Oura API token - can be overridden via OURA_TOKEN env var
fn get_oura_token() -> String {
    std::env::var("OURA_TOKEN").unwrap_or_else(|_| "527UFS4RVNQA4R72IIAGNHWMCQZ7A6EU".to_string())
}

/// MIME types for common file extensions.
fn mime_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Parse query string into HashMap
fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            params.insert(key.to_string(), urlencoded_decode(value));
        }
    }
    params
}

/// Simple URL decoding
fn urlencoded_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Read HTTP request body
fn read_body(stream: &mut TcpStream, headers: &str) -> Option<String> {
    let content_length: usize = headers
        .lines()
        .find(|line| line.to_lowercase().starts_with("content-length:"))
        .and_then(|line| line.split(':').nth(1))
        .and_then(|len| len.trim().parse().ok())
        .unwrap_or(0);
    
    if content_length == 0 {
        return None;
    }
    
    let body_start = headers.find("\r\n\r\n").map(|i| i + 4).unwrap_or(headers.len());
    let body_from_headers = if body_start < headers.len() {
        headers[body_start..].to_string()
    } else {
        String::new()
    };
    
    if body_from_headers.len() >= content_length {
        return Some(body_from_headers[..content_length].to_string());
    }
    
    let remaining = content_length - body_from_headers.len();
    let mut body_buf = vec![0u8; remaining];
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    
    match stream.read_exact(&mut body_buf) {
        Ok(_) => {
            let mut full_body = body_from_headers;
            full_body.push_str(&String::from_utf8_lossy(&body_buf));
            Some(full_body)
        }
        Err(_) => Some(body_from_headers),
    }
}

/// Handle an HTTP request.
/// Returns true if handled, false if WebSocket upgrade needed.
pub fn handle_request(stream: &mut TcpStream, request: &str, public_dir: &str) -> bool {
    if request.contains("Upgrade: websocket") || request.contains("upgrade: websocket") {
        return false;
    }
    
    let request_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    
    if parts.len() < 2 {
        send_error(stream, 400, "Bad Request");
        return true;
    }
    
    let method = parts[0];
    let full_path = parts[1];
    let (path, query) = full_path.split_once('?').unwrap_or((full_path, ""));
    let query_params = parse_query_string(query);
    
    // Route API calls
    if path.starts_with("/api/") {
        let db = match StateDb::open("data/state.db") {
            Ok(db) => db,
            Err(e) => {
                send_json_error(stream, 500, &format!("Database error: {}", e));
                return true;
            }
        };
        handle_api(stream, method, path, query, &query_params, request, &db);
        return true;
    }
    
    if method != "GET" {
        send_error(stream, 405, "Method Not Allowed");
        return true;
    }
    
    let path = if path == "/" { "/globalui.html" } else { path };
    let path = path.replace("..", "");
    let file_path = format!("{}{}", public_dir, path);
    let file_path = Path::new(&file_path);
    
    if !file_path.starts_with(public_dir) {
        send_error(stream, 403, "Forbidden");
        return true;
    }
    
    match fs::read(&file_path) {
        Ok(content) => {
            let mime = mime_type(&path);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
                mime, content.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&content);
        }
        Err(_) => send_error(stream, 404, "Not Found"),
    }
    
    true
}

/// Handle API requests
fn handle_api(
    stream: &mut TcpStream, 
    method: &str, 
    path: &str, 
    query: &str,
    query_params: &HashMap<String, String>,
    request: &str,
    db: &StateDb,
) {
    if method == "OPTIONS" {
        send_cors_preflight(stream);
        return;
    }
    
    match (method, path) {
        // Pairing requests list
        ("GET", "/api/pair/requests") => {
            match db.get_pending_pairing_requests() {
                Ok(requests) => {
                    let json: Vec<serde_json::Value> = requests.iter().map(|r| {
                        serde_json::json!({
                            "device_id": r.device_id,
                            "name": r.name,
                            "device_type": r.device_type,
                            "code": r.code,
                            "expires_at": r.expires_at,
                            "created_at": r.created_at
                        })
                    }).collect();
                    send_json(stream, 200, &serde_json::json!({"requests": json}));
                }
                Err(e) => send_json_error(stream, 500, &e),
            }
        }
        
        // Device requests to join
        ("POST", "/api/pair/request") => {
            let body = match read_body(stream, request) {
                Some(b) => b,
                None => { send_json_error(stream, 400, "Missing body"); return; }
            };
            
            let data: serde_json::Value = match serde_json::from_str(&body) {
                Ok(d) => d,
                Err(_) => { send_json_error(stream, 400, "Invalid JSON"); return; }
            };
            
            let device_id = data.get("device_id").and_then(|v| v.as_str()).unwrap_or("");
            let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown Device");
            let device_type = data.get("device_type").and_then(|v| v.as_str()).unwrap_or("unknown");
            
            if device_id.is_empty() {
                send_json_error(stream, 400, "device_id required");
                return;
            }
            
            match db.create_pairing_request(device_id, name, device_type) {
                Ok(code) => {
                    println!("🔔 Pairing request: {} ({}) - Code: {}", name, device_id, code);
                    send_json(stream, 200, &serde_json::json!({
                        "status": "pending",
                        "message": "Enter the 6-digit code shown in GlobalUI",
                        "device_id": device_id
                    }));
                }
                Err(e) => send_json_error(stream, 500, &e),
            }
        }
        
        // Device confirms with code
        ("POST", "/api/pair/confirm") => {
            let body = match read_body(stream, request) {
                Some(b) => b,
                None => { send_json_error(stream, 400, "Missing body"); return; }
            };
            
            let data: serde_json::Value = match serde_json::from_str(&body) {
                Ok(d) => d,
                Err(_) => { send_json_error(stream, 400, "Invalid JSON"); return; }
            };
            
            let device_id = data.get("device_id").and_then(|v| v.as_str()).unwrap_or("");
            let code = data.get("code").and_then(|v| v.as_str()).unwrap_or("");
            
            if device_id.is_empty() || code.is_empty() {
                send_json_error(stream, 400, "device_id and code required");
                return;
            }
            
            match db.confirm_pairing(device_id, &code.to_uppercase()) {
                Ok(token) => {
                    println!("✓ Device paired: {}", device_id);
                    send_json(stream, 200, &serde_json::json!({
                        "status": "paired",
                        "token": token,
                        "device_id": device_id
                    }));
                }
                Err(e) => send_json_error(stream, 400, &e),
            }
        }
        
        // Devices list
        ("GET", "/api/devices") => {
            match db.get_all_devices() {
                Ok(devices) => {
                    let json: Vec<serde_json::Value> = devices.iter().map(|d| {
                        serde_json::json!({
                            "id": d.id,
                            "name": d.name,
                            "device_type": d.device_type,
                            "status": d.status,
                            "latitude": d.latitude,
                            "longitude": d.longitude,
                            "battery": d.battery,
                            "last_seen": d.last_seen
                        })
                    }).collect();
                    send_json(stream, 200, &serde_json::json!({"devices": json}));
                }
                Err(e) => send_json_error(stream, 500, &e),
            }
        }
        
        // Oura API proxy - handles all /api/oura/* paths
        _ if method == "GET" && path.starts_with("/api/oura/") => {
            // Extract the Oura API path (everything after /api/oura)
            let oura_path = path.trim_start_matches("/api/oura");
            match fetch_oura_api(oura_path, query) {
                Ok(data) => send_json(stream, 200, &data),
                Err(e) => send_json_error(stream, 502, &e),
            }
        }
        
        // Delete pairing request or device
        _ if method == "DELETE" && path.starts_with("/api/pair/") => {
            let device_id = path.trim_start_matches("/api/pair/");
            match db.delete_pairing_request(device_id) {
                Ok(_) => send_json(stream, 200, &serde_json::json!({"status": "deleted"})),
                Err(e) => send_json_error(stream, 500, &e),
            }
        }
        
        _ if method == "DELETE" && path.starts_with("/api/devices/") => {
            let device_id = path.trim_start_matches("/api/devices/");
            match db.delete_device(device_id) {
                Ok(_) => {
                    println!("✗ Device revoked: {}", device_id);
                    send_json(stream, 200, &serde_json::json!({"status": "deleted"}));
                }
                Err(e) => send_json_error(stream, 500, &e),
            }
        }
        
        _ => send_json_error(stream, 404, "Not found"),
    }
}

/// Fetch data from Oura API via HTTPS
/// Uses rustls for TLS - pure Rust, no OpenSSL dependency
fn fetch_oura_api(path: &str, query: &str) -> Result<serde_json::Value, String> {
    // Build full URL path with query string
    let full_path = if query.is_empty() {
        path.to_string()
    } else {
        format!("{}?{}", path, query)
    };
    
    // Use the system's curl command for HTTPS (simplest approach)
    // This avoids adding rustls/native-tls dependencies while still working
    let token = get_oura_token();
    let url = format!("https://api.ouraring.com{}", full_path);
    
    // Try curl first (available on most systems)
    let output = std::process::Command::new("curl")
        .args([
            "-s",
            "-H", &format!("Authorization: Bearer {}", token),
            "-H", "Accept: application/json",
            &url
        ])
        .output();
    
    match output {
        Ok(output) if output.status.success() => {
            let body = String::from_utf8_lossy(&output.stdout);
            serde_json::from_str(&body)
                .map_err(|e| format!("JSON parse error: {}", e))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("curl failed: {}", stderr))
        }
        Err(_) => {
            // curl not available - try wget as fallback
            let output = std::process::Command::new("wget")
                .args([
                    "-q", "-O", "-",
                    "--header", &format!("Authorization: Bearer {}", token),
                    "--header", "Accept: application/json",
                    &url
                ])
                .output();
            
            match output {
                Ok(output) if output.status.success() => {
                    let body = String::from_utf8_lossy(&output.stdout);
                    serde_json::from_str(&body)
                        .map_err(|e| format!("JSON parse error: {}", e))
                }
                Ok(_) => Err("wget failed to fetch Oura API".to_string()),
                Err(_) => Err("Neither curl nor wget available. Install curl for Oura API support.".to_string()),
            }
        }
    }
}

/// Send JSON response
fn send_json(stream: &mut TcpStream, status: u16, data: &serde_json::Value) {
    let body = serde_json::to_string(data).unwrap_or_default();
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "Unknown",
    };
    
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n{}",
        status, status_text, body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Send JSON error response
fn send_json_error(stream: &mut TcpStream, status: u16, message: &str) {
    send_json(stream, status, &serde_json::json!({"error": message}));
}

/// Send CORS preflight response
fn send_cors_preflight(stream: &mut TcpStream) {
    let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Max-Age: 86400\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
}

/// Send an HTTP error response.
fn send_error(stream: &mut TcpStream, code: u16, message: &str) {
    let body = format!("<h1>{} {}</h1>", code, message);
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code, message, body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Read HTTP request from stream (up to headers).
pub fn read_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut buffer = [0u8; 8192];
    let mut request = String::new();
    
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                request.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if request.contains("\r\n\r\n") {
                    break;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    
    Ok(request)
}
