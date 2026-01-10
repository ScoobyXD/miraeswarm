//! # HTTP Server
//! 
//! Minimal HTTP server for serving static files.
//! 
//! This is NOT a general-purpose HTTP server. It does exactly one thing:
//! serve files from the public/ directory to browsers.
//! 
//! WHY FROM SCRATCH:
//! - We need ~100 lines, not a framework
//! - Static file serving is trivial
//! - No dependency that can break

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

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

/// Handle an HTTP request for a static file.
/// Returns true if handled (was HTTP), false if not HTTP (probably WebSocket upgrade).
pub fn handle_request(stream: &mut TcpStream, request: &str, public_dir: &str) -> bool {
    // Check if this is a WebSocket upgrade request
    if request.contains("Upgrade: websocket") || request.contains("upgrade: websocket") {
        return false; // Let WebSocket handler deal with it
    }
    
    // Parse request line: "GET /path HTTP/1.1"
    let request_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    
    if parts.len() < 2 {
        send_error(stream, 400, "Bad Request");
        return true;
    }
    
    let method = parts[0];
    let mut path = parts[1];
    
    // Only handle GET
    if method != "GET" {
        send_error(stream, 405, "Method Not Allowed");
        return true;
    }
    
    // Default to index
    if path == "/" {
        path = "/globalui.html";
    }
    
    // Remove query string
    let path = path.split('?').next().unwrap_or(path);
    
    // Security: prevent directory traversal
    let path = path.replace("..", "");
    
    // Build file path
    let file_path = format!("{}{}", public_dir, path);
    let file_path = Path::new(&file_path);
    
    // Check if file exists and is within public dir
    if !file_path.starts_with(public_dir) {
        send_error(stream, 403, "Forbidden");
        return true;
    }
    
    // Read and serve file
    match fs::read(&file_path) {
        Ok(content) => {
            let mime = mime_type(&path);
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: {}\r\n\
                 Content-Length: {}\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 Connection: close\r\n\r\n",
                mime,
                content.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&content);
        }
        Err(_) => {
            send_error(stream, 404, "Not Found");
        }
    }
    
    true
}

/// Send an HTTP error response.
fn send_error(stream: &mut TcpStream, code: u16, message: &str) {
    let body = format!("<h1>{} {}</h1>", code, message);
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: text/html\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {}",
        code,
        message,
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Read HTTP request from stream (up to headers).
pub fn read_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut buffer = [0u8; 4096];
    let mut request = String::new();
    
    // Set timeout for initial read
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
