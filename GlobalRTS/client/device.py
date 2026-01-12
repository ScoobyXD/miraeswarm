#!/usr/bin/env python3
"""
GlobalRTS Device Client

Zero-dependency Python client for connecting devices to GlobalRTS.
Uses only Python standard library.

USAGE:
    python device.py                    # Interactive mode
    python device.py --server HOST:PORT # Specify server
    python device.py --id my-robot      # Set device ID
    python device.py --name "My Robot"  # Set device name
    python device.py --type robot       # Set device type

WORKFLOW:
    1. Script requests to pair with server
    2. Server generates 6-digit code, shows in GlobalUI
    3. You enter the code when prompted
    4. Script receives auth token, saves to device_token.txt
    5. Script connects via WebSocket
    6. Script sends telemetry, receives commands

Subsequent runs use the saved token automatically.
"""

import json
import socket
import ssl
import hashlib
import base64
import struct
import os
import sys
import time
import random
import argparse
from urllib.parse import urlparse

# ============================================================================
# CONFIGURATION
# ============================================================================

DEFAULT_SERVER = "localhost:3000"
TOKEN_FILE = "device_token.txt"
TELEMETRY_INTERVAL = 1.0  # seconds

# ============================================================================
# HTTP CLIENT (minimal, stdlib only)
# ============================================================================

def http_request(method, url, body=None, headers=None):
    """Make an HTTP request. Returns (status_code, response_body)."""
    parsed = urlparse(url)
    host = parsed.hostname
    port = parsed.port or (443 if parsed.scheme == "https" else 80)
    path = parsed.path or "/"
    if parsed.query:
        path += "?" + parsed.query
    
    # Connect
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(10)
    
    if parsed.scheme == "https":
        context = ssl.create_default_context()
        sock = context.wrap_socket(sock, server_hostname=host)
    
    sock.connect((host, port))
    
    # Build request
    headers = headers or {}
    headers["Host"] = host
    headers["Connection"] = "close"
    
    if body:
        if isinstance(body, dict):
            body = json.dumps(body)
        headers["Content-Type"] = "application/json"
        headers["Content-Length"] = str(len(body))
    
    request = f"{method} {path} HTTP/1.1\r\n"
    for k, v in headers.items():
        request += f"{k}: {v}\r\n"
    request += "\r\n"
    
    sock.sendall(request.encode())
    if body:
        sock.sendall(body.encode())
    
    # Read response
    response = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            break
        response += chunk
    
    sock.close()
    
    # Parse response
    response = response.decode("utf-8", errors="replace")
    header_end = response.find("\r\n\r\n")
    if header_end == -1:
        return 0, ""
    
    status_line = response.split("\r\n")[0]
    status_code = int(status_line.split()[1])
    body = response[header_end + 4:]
    
    # Handle chunked encoding (simple)
    if "Transfer-Encoding: chunked" in response[:header_end]:
        # Just try to parse the JSON directly
        pass
    
    return status_code, body

# ============================================================================
# WEBSOCKET CLIENT (minimal, RFC 6455)
# ============================================================================

class WebSocketClient:
    """Minimal WebSocket client implementing RFC 6455."""
    
    def __init__(self, url):
        parsed = urlparse(url)
        self.host = parsed.hostname
        self.port = parsed.port or (443 if parsed.scheme in ("wss", "https") else 80)
        self.use_ssl = parsed.scheme in ("wss", "https")
        self.path = parsed.path or "/"
        self.sock = None
    
    def connect(self):
        """Perform WebSocket handshake."""
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.settimeout(10)
        
        if self.use_ssl:
            context = ssl.create_default_context()
            self.sock = context.wrap_socket(self.sock, server_hostname=self.host)
        
        self.sock.connect((self.host, self.port))
        
        # Generate random key
        key = base64.b64encode(os.urandom(16)).decode()
        
        # Send upgrade request
        request = (
            f"GET {self.path} HTTP/1.1\r\n"
            f"Host: {self.host}:{self.port}\r\n"
            f"Upgrade: websocket\r\n"
            f"Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            f"Sec-WebSocket-Version: 13\r\n"
            f"\r\n"
        )
        self.sock.sendall(request.encode())
        
        # Read response
        response = b""
        while b"\r\n\r\n" not in response:
            chunk = self.sock.recv(1024)
            if not chunk:
                raise ConnectionError("Connection closed during handshake")
            response += chunk
        
        if b"101" not in response:
            raise ConnectionError(f"WebSocket upgrade failed: {response.decode()}")
        
        self.sock.setblocking(False)
        return True
    
    def send(self, message):
        """Send a text message."""
        if isinstance(message, dict):
            message = json.dumps(message)
        
        payload = message.encode()
        length = len(payload)
        
        # Build frame
        frame = bytearray()
        frame.append(0x81)  # FIN + TEXT opcode
        
        # Length + mask bit
        if length < 126:
            frame.append(0x80 | length)
        elif length < 65536:
            frame.append(0x80 | 126)
            frame.extend(struct.pack(">H", length))
        else:
            frame.append(0x80 | 127)
            frame.extend(struct.pack(">Q", length))
        
        # Masking key
        mask = os.urandom(4)
        frame.extend(mask)
        
        # Masked payload
        for i, byte in enumerate(payload):
            frame.append(byte ^ mask[i % 4])
        
        self.sock.setblocking(True)
        self.sock.sendall(bytes(frame))
        self.sock.setblocking(False)
    
    def recv(self, timeout=0.1):
        """Receive a message. Returns None if no message available."""
        self.sock.settimeout(timeout)
        
        try:
            # Read header
            header = self.sock.recv(2)
            if len(header) < 2:
                return None
            
            opcode = header[0] & 0x0F
            length = header[1] & 0x7F
            
            # Extended length
            if length == 126:
                ext = self.sock.recv(2)
                length = struct.unpack(">H", ext)[0]
            elif length == 127:
                ext = self.sock.recv(8)
                length = struct.unpack(">Q", ext)[0]
            
            # Read payload
            payload = b""
            while len(payload) < length:
                chunk = self.sock.recv(length - len(payload))
                if not chunk:
                    break
                payload += chunk
            
            if opcode == 0x1:  # Text
                return payload.decode()
            elif opcode == 0x8:  # Close
                self.close()
                return None
            elif opcode == 0x9:  # Ping
                self._send_pong(payload)
                return None
            
            return None
            
        except (socket.timeout, BlockingIOError):
            return None
        except Exception as e:
            print(f"WebSocket recv error: {e}")
            return None
    
    def _send_pong(self, payload):
        """Respond to ping."""
        frame = bytearray([0x8A, 0x80 | len(payload)])
        mask = os.urandom(4)
        frame.extend(mask)
        for i, byte in enumerate(payload):
            frame.append(byte ^ mask[i % 4])
        self.sock.sendall(bytes(frame))
    
    def close(self):
        """Close connection."""
        if self.sock:
            try:
                self.sock.close()
            except:
                pass
            self.sock = None

# ============================================================================
# DEVICE STATE
# ============================================================================

class DeviceState:
    """Simulated device state. Replace with real sensor readings."""
    
    def __init__(self, lat=34.0522, lon=-118.2437):
        self.lat = lat + (random.random() - 0.5) * 0.01
        self.lon = lon + (random.random() - 0.5) * 0.01
        self.alt = 0.0
        self.heading = random.random() * 360
        self.speed = 0.0
        self.battery = 85 + random.random() * 15
        self.target = None
    
    def update(self):
        """Update state (simulate movement if target set)."""
        if self.target:
            target_lat, target_lon = self.target
            dlat = target_lat - self.lat
            dlon = target_lon - self.lon
            dist = (dlat**2 + dlon**2)**0.5
            
            if dist < 0.0001:
                self.lat = target_lat
                self.lon = target_lon
                self.speed = 0.0
                self.target = None
                print("   ✓ Arrived at destination")
            else:
                step = 0.0002
                self.lat += (dlat / dist) * step
                self.lon += (dlon / dist) * step
                self.heading = (dlon / dist) * 90  # Simplified
                self.speed = step * 111000
        
        # Drain battery slowly
        self.battery = max(0, self.battery - 0.001)
    
    def to_telemetry(self):
        return {
            "latitude": self.lat,
            "longitude": self.lon,
            "altitude": self.alt,
            "heading": self.heading,
            "speed": self.speed,
            "battery": self.battery,
            "sensors": {}
        }

# ============================================================================
# PAIRING FLOW
# ============================================================================

def pair_device(server, device_id, name, device_type):
    """
    Pair device with server using 6-digit code flow.
    Returns auth token on success.
    """
    base_url = f"http://{server}"
    
    # Step 1: Request to pair
    print(f"\n📡 Requesting to pair with {server}...")
    status, body = http_request(
        "POST",
        f"{base_url}/api/pair/request",
        body={
            "device_id": device_id,
            "name": name,
            "device_type": device_type
        }
    )
    
    if status != 200:
        print(f"❌ Failed to request pairing: {body}")
        return None
    
    response = json.loads(body)
    if response.get("status") != "pending":
        print(f"❌ Unexpected response: {response}")
        return None
    
    print(f"✓ Pairing request sent")
    print(f"\n" + "="*50)
    print(f"📱 Look at GlobalUI for the 6-digit code")
    print(f"="*50 + "\n")
    
    # Step 2: Wait for user to enter code
    while True:
        code = input("Enter 6-digit code: ").strip().upper()
        if len(code) == 6:
            break
        print("Code must be 6 characters")
    
    # Step 3: Confirm with code
    print(f"\n🔐 Confirming code {code}...")
    status, body = http_request(
        "POST",
        f"{base_url}/api/pair/confirm",
        body={
            "device_id": device_id,
            "code": code
        }
    )
    
    if status != 200:
        try:
            error = json.loads(body).get("error", body)
        except:
            error = body
        print(f"❌ Pairing failed: {error}")
        return None
    
    response = json.loads(body)
    token = response.get("token")
    
    if token:
        print(f"✓ Paired successfully!")
        return token
    else:
        print(f"❌ No token in response: {response}")
        return None

def load_token():
    """Load saved token from file."""
    if os.path.exists(TOKEN_FILE):
        with open(TOKEN_FILE, "r") as f:
            data = json.load(f)
            return data.get("token"), data.get("device_id")
    return None, None

def save_token(token, device_id):
    """Save token to file."""
    with open(TOKEN_FILE, "w") as f:
        json.dump({"token": token, "device_id": device_id}, f)

# ============================================================================
# COMMAND HANDLING
# ============================================================================

def handle_command(state, command_type, payload):
    """Handle command from server."""
    print(f"\n📥 Command: {command_type}")
    
    if command_type == "navigate":
        lat = payload.get("latitude", state.lat)
        lon = payload.get("longitude", state.lon)
        state.target = (lat, lon)
        print(f"   🚀 Navigating to {lat:.6f}, {lon:.6f}")
        return "received"
    
    elif command_type == "stop":
        state.target = None
        state.speed = 0
        print(f"   🛑 Stopped")
        return "completed"
    
    elif command_type == "ring":
        print(f"   🔔 RING RING RING!")
        return "completed"
    
    else:
        print(f"   ❓ Unknown command")
        return "unknown"

# ============================================================================
# MAIN
# ============================================================================

def main():
    parser = argparse.ArgumentParser(description="GlobalRTS Device Client")
    parser.add_argument("--server", default=DEFAULT_SERVER, help="Server host:port")
    parser.add_argument("--id", default=None, help="Device ID")
    parser.add_argument("--name", default="Python Device", help="Device name")
    parser.add_argument("--type", default="robot", help="Device type")
    parser.add_argument("--reset", action="store_true", help="Clear saved token and re-pair")
    args = parser.parse_args()
    
    # Generate device ID if not provided
    device_id = args.id
    if not device_id:
        device_id = f"device-{int(time.time()) % 100000:05d}"
    
    print("\n" + "="*50)
    print("  GLOBALRTS DEVICE CLIENT")
    print("="*50)
    print(f"  Server: {args.server}")
    print(f"  Device: {args.name} ({device_id})")
    print(f"  Type:   {args.type}")
    print("="*50 + "\n")
    
    # Check for saved token
    token, saved_id = load_token()
    
    if args.reset or not token:
        # Need to pair
        token = pair_device(args.server, device_id, args.name, args.type)
        if not token:
            print("\n❌ Could not pair device. Exiting.")
            sys.exit(1)
        save_token(token, device_id)
        print(f"✓ Token saved to {TOKEN_FILE}")
    else:
        device_id = saved_id or device_id
        print(f"✓ Using saved token for {device_id}")
    
    # Connect WebSocket
    ws_url = f"ws://{args.server}/"
    print(f"\n🔌 Connecting to {ws_url}...")
    
    ws = WebSocketClient(ws_url)
    try:
        ws.connect()
        print("✓ WebSocket connected")
    except Exception as e:
        print(f"❌ Failed to connect: {e}")
        sys.exit(1)
    
    # Register with token
    print("📝 Registering device...")
    ws.send({
        "type": "register",
        "data": {
            "token": token,
            "device_id": device_id,
            "device_type": args.type,
            "name": args.name,
            "latitude": 34.0522,
            "longitude": -118.2437
        }
    })
    
    # Wait for registration confirmation
    time.sleep(0.5)
    response = ws.recv(timeout=2.0)
    if response:
        msg = json.loads(response)
        if msg.get("type") == "error":
            print(f"❌ Registration failed: {msg.get('data', {}).get('message')}")
            if "token" in msg.get("data", {}).get("message", "").lower():
                print("   Token may be invalid. Try running with --reset to re-pair.")
            sys.exit(1)
        elif msg.get("type") == "registered":
            print("✓ Registered successfully!")
        else:
            print(f"   Got: {msg}")
    
    print(f"\n🤖 Device is live! Sending telemetry every {TELEMETRY_INTERVAL}s")
    print("   Press Ctrl+C to stop\n")
    
    # Initialize state
    state = DeviceState()
    tick = 0
    
    # Main loop
    try:
        while True:
            # Check for commands
            msg = ws.recv(timeout=0.1)
            if msg:
                try:
                    envelope = json.loads(msg)
                    if envelope.get("type") == "command":
                        data = envelope.get("data", {})
                        command_id = data.get("commandId", "")
                        command_type = data.get("type", "")
                        payload = data.get("payload", {})
                        
                        status = handle_command(state, command_type, payload)
                        
                        # Send ack
                        ws.send({
                            "type": "command:ack",
                            "data": {
                                "commandId": command_id,
                                "status": status
                            }
                        })
                except json.JSONDecodeError:
                    pass
            
            # Update state
            state.update()
            
            # Send telemetry
            ws.send({
                "type": "telemetry",
                "data": state.to_telemetry()
            })
            
            # Log status periodically
            tick += 1
            if tick % 10 == 0:
                print(f"📍 {state.lat:.6f}, {state.lon:.6f} | 🔋 {state.battery:.1f}%")
            
            time.sleep(TELEMETRY_INTERVAL)
            
    except KeyboardInterrupt:
        print("\n\n👋 Shutting down...")
    finally:
        ws.close()

if __name__ == "__main__":
    main()
