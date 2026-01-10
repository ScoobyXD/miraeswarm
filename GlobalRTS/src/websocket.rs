//! # WebSocket Implementation
//! 
//! RFC 6455 WebSocket protocol, written from scratch.
//! 
//! WHY FROM SCRATCH:
//! - RFC 6455 hasn't changed since 2011. Won't change.
//! - ~300 lines vs external library's thousands
//! - No dependency that can break or change
//! - Any AI can read and understand this completely
//!
//! IMPLEMENTS:
//! - HTTP upgrade handshake
//! - Text frame encoding/decoding
//! - Ping/pong for keepalive
//! - Clean close handshake
//! - Client masking (required by spec)

use std::io::{Read, Write};
use std::net::TcpStream;
use sha1::{Sha1, Digest};
use base64::Engine;

/// WebSocket GUID from RFC 6455. This is a magic constant that never changes.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Frame opcodes from RFC 6455
const OPCODE_TEXT: u8 = 0x1;
const OPCODE_CLOSE: u8 = 0x8;
const OPCODE_PING: u8 = 0x9;
const OPCODE_PONG: u8 = 0xA;

/// WebSocket connection state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Open,
    Closing,
    Closed,
}

/// A WebSocket connection wrapping a TCP stream.
pub struct WebSocket {
    stream: TcpStream,
    pub state: State,
}

#[allow(dead_code)]
impl WebSocket {
    /// Perform server-side WebSocket handshake.
    /// Takes a TCP stream that has received an HTTP upgrade request.
    pub fn accept(mut stream: TcpStream, request: &str) -> Result<Self, String> {
        // Extract Sec-WebSocket-Key from request headers
        let key = request
            .lines()
            .find(|line| line.to_lowercase().starts_with("sec-websocket-key:"))
            .and_then(|line| line.split(':').nth(1))
            .map(|k| k.trim())
            .ok_or("Missing Sec-WebSocket-Key")?;
        
        // Calculate accept key: base64(sha1(key + GUID))
        let mut hasher = Sha1::new();
        hasher.update(key.as_bytes());
        hasher.update(WS_GUID.as_bytes());
        let hash = hasher.finalize();
        let accept = base64::engine::general_purpose::STANDARD.encode(hash);
        
        // Send upgrade response
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            accept
        );
        
        stream.write_all(response.as_bytes()).map_err(|e| e.to_string())?;
        stream.set_nonblocking(true).map_err(|e| e.to_string())?;
        
        Ok(Self {
            stream,
            state: State::Open,
        })
    }
    
    /// Read a message from the WebSocket.
    /// Returns None if no complete message available (non-blocking).
    /// Returns Some(message) for text messages.
    /// Handles ping/pong automatically.
    pub fn read(&mut self) -> Result<Option<String>, String> {
        if self.state != State::Open {
            return Ok(None);
        }
        
        // Try to read frame header (2 bytes minimum)
        let mut header = [0u8; 2];
        match self.stream.read_exact(&mut header) {
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(e) => {
                self.state = State::Closed;
                return Err(e.to_string());
            }
        }
        
        let _fin = (header[0] & 0x80) != 0;
        let opcode = header[0] & 0x0F;
        let masked = (header[1] & 0x80) != 0;
        let mut payload_len = (header[1] & 0x7F) as usize;
        
        // Extended payload length
        if payload_len == 126 {
            let mut ext = [0u8; 2];
            self.stream.read_exact(&mut ext).map_err(|e| e.to_string())?;
            payload_len = u16::from_be_bytes(ext) as usize;
        } else if payload_len == 127 {
            let mut ext = [0u8; 8];
            self.stream.read_exact(&mut ext).map_err(|e| e.to_string())?;
            payload_len = u64::from_be_bytes(ext) as usize;
        }
        
        // Read masking key (client messages are always masked)
        let mask = if masked {
            let mut m = [0u8; 4];
            self.stream.read_exact(&mut m).map_err(|e| e.to_string())?;
            Some(m)
        } else {
            None
        };
        
        // Read payload
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            self.stream.read_exact(&mut payload).map_err(|e| e.to_string())?;
        }
        
        // Unmask if needed
        if let Some(mask) = mask {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }
        }
        
        // Handle by opcode
        match opcode {
            OPCODE_TEXT => {
                let text = String::from_utf8(payload).map_err(|e| e.to_string())?;
                Ok(Some(text))
            }
            OPCODE_CLOSE => {
                self.state = State::Closing;
                // Echo close frame
                let _ = self.write_frame(&payload, OPCODE_CLOSE);
                self.state = State::Closed;
                Ok(None)
            }
            OPCODE_PING => {
                // Respond with pong
                let _ = self.write_frame(&payload, OPCODE_PONG);
                Ok(None)
            }
            OPCODE_PONG => Ok(None), // Ignore pongs
            _ => Ok(None), // Ignore unknown opcodes
        }
    }
    
    /// Send a text message.
    pub fn send(&mut self, message: &str) -> Result<(), String> {
        if self.state != State::Open {
            return Err("Connection not open".to_string());
        }
        self.write_frame(message.as_bytes(), OPCODE_TEXT)
    }
    
    /// Write a WebSocket frame. Server frames are NOT masked.
    fn write_frame(&mut self, payload: &[u8], opcode: u8) -> Result<(), String> {
        let len = payload.len();
        let mut frame = Vec::with_capacity(10 + len);
        
        // First byte: FIN + opcode
        frame.push(0x80 | opcode);
        
        // Second byte: length (no mask bit for server->client)
        if len < 126 {
            frame.push(len as u8);
        } else if len < 65536 {
            frame.push(126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        
        // Payload (unmasked)
        frame.extend_from_slice(payload);
        
        self.stream.write_all(&frame).map_err(|e| e.to_string())
    }
    
    /// Close the connection gracefully.
    pub fn close(&mut self) {
        if self.state == State::Open {
            self.state = State::Closing;
            let _ = self.write_frame(&[], OPCODE_CLOSE);
            self.state = State::Closed;
        }
    }
    
    /// Get the peer address.
    pub fn peer_addr(&self) -> String {
        self.stream.peer_addr().map(|a| a.to_string()).unwrap_or_default()
    }
    
    /// Clone the underlying stream for the client registry.
    pub fn try_clone(&self) -> Result<WebSocket, String> {
        Ok(WebSocket {
            stream: self.stream.try_clone().map_err(|e| e.to_string())?,
            state: self.state,
        })
    }
}
