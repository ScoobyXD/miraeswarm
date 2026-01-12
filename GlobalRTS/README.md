# GlobalRTS

Command center for robot fleets. Single binary. Zero runtime dependencies. 1000-year-proof.

```
    ╔═══════════════════════════════════════════════════════════════════╗
    ║                         GlobalRTS                                 ║
    ║                                                                   ║
    ║     "Turn everything into a real-life RTS game"                   ║
    ║                                                                   ║
    ║     🌍 3D Globe  →  See all your devices worldwide                ║
    ║     🤖 Control   →  Select units, give commands                   ║
    ║     📡 Data      →  Live telemetry, sensors, video               ║
    ║     🔧 Reprogram →  Change any device remotely                   ║
    ╚═══════════════════════════════════════════════════════════════════╝
```

## Quick Start

```bash
# Build (requires Rust)
cargo build --release

# Run server
./target/release/globalrts

# Open browser
# http://localhost:3000

# Connect a device (Python example)
cd client
python device.py
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         YOUR BROWSER (GlobalUI)                        │
│                                                                         │
│   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│   │  3D Globe   │  │   Device    │  │   Device    │  │   Health    │  │
│   │  (CesiumJS) │  │   Pairing   │  │   Manager   │  │  (Oura)     │  │
│   └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ WebSocket + HTTP
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      GlobalRTS SERVER (Single Rust Binary)              │
│                                                                         │
│   Port 3000 ─────────────────────────────────────────────────────────  │
│     │                                                                   │
│     ├── GET  /                     → GlobalUI (static HTML/JS)         │
│     ├── GET  /api/oura/sleep       → Proxy to Oura Ring API            │
│     ├── POST /api/pair/request     → Device requests to join           │
│     ├── POST /api/pair/confirm     → Device confirms 6-digit code      │
│     ├── GET  /api/devices          → List paired devices               │
│     ├── DELETE /api/devices/{id}   → Revoke device                     │
│     │                                                                   │
│     └── WebSocket /ws              → Real-time communication           │
│           ├── Device telemetry                                          │
│           ├── Commands to devices                                       │
│           └── UI updates                                                │
│                                                                         │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                │
│   │   SQLite     │  │  Telemetry   │  │   Pairing    │                │
│   │  (state.db)  │  │   (JSONL)    │  │   (tokens)   │                │
│   └──────────────┘  └──────────────┘  └──────────────┘                │
└─────────────────────────────────────────────────────────────────────────┘
                                    ▲
                                    │ WebSocket (outbound from device)
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐          ┌───────────────┐          ┌───────────────┐
│    Robot      │          │    Phone      │          │  IoT Sensor   │
│  (Raspberry   │          │   (PWA or     │          │   (ESP32)     │
│     Pi)       │          │    App)       │          │               │
│               │          │               │          │               │
│  Python       │          │  JavaScript   │          │  C/Python     │
│  device.py    │          │  or Native    │          │               │
└───────────────┘          └───────────────┘          └───────────────┘
```

## Philosophy

See [PHILOSOPHY.md](PHILOSOPHY.md) for the full manifesto.

**TL;DR:**
- Observable: Every byte of data inspectable
- Reprogrammable: Change anything remotely
- 1000-year-proof: No external dependencies that can break
- Just run it: Single binary, no setup

### On Dependencies

**Core application** (MUST be 1000-year-proof):
- Server binary: Zero runtime dependencies. Rust compiles to static binary.
- WebSocket protocol: RFC 6455, implemented from scratch (~300 lines)
- Database: SQLite, embedded in binary
- Telemetry: Flat JSONL files

**Feature integrations** (OK to depend on external APIs):
- Oura Ring health data: Nice-to-have feature, not core. If Oura API breaks, health panel stops working, robots keep running.
- News feeds (RSS): Informational overlay. If rss2json dies, news stops, robots keep running.
- CesiumJS + Google 3D Tiles: **Necessary evil.** We don't have satellites. When we do, we'll serve our own imagery. The core WebSocket/device communication works without Cesium - you just won't see the pretty globe.

**The rule:** If a dependency breaks, does the robot fleet stop working? If yes, we can't use it. If no, it's an acceptable feature integration.

### Why NOT These Alternatives

| Technology | Why We Don't Use It |
|------------|---------------------|
| Nginx | External service, config files, can break on updates |
| Mosquitto/MQTT | Another broker to install and maintain |
| Docker | Abstraction layer, version conflicts, not necessary |
| WebRTC (webrtc-rs) | 200k+ lines, complex, browser implementations change |
| Janus/Mediasoup | External media servers, violates single-binary |
| Cloud services | Vendor lock-in, monthly costs, dependency |

### What We DO Use

| Technology | Why |
|------------|-----|
| Raw TCP/WebSocket | RFC 6455 hasn't changed since 2011, won't change |
| HTTP/1.1 | The simplest protocol, works everywhere |
| SQLite | 20+ years stable, embedded in binary |
| JSONL files | Plain text, any tool can read it |
| MJPEG (future) | Works in `<img>` tag since 1999 |

## Device Pairing Flow

Devices don't need pre-configured tokens. They request to join, you approve.

```
┌──────────────┐                    ┌──────────────┐                    ┌──────────────┐
│   Device     │                    │    Server    │                    │   GlobalUI   │
└──────┬───────┘                    └──────┬───────┘                    └──────┬───────┘
       │                                   │                                   │
       │ POST /api/pair/request            │                                   │
       │ {device_id, name, type}           │                                   │
       │ ─────────────────────────────────▶│                                   │
       │                                   │                                   │
       │                                   │ Generate code "A7X9K2"            │
       │                                   │ Start 5-minute timer              │
       │                                   │                                   │
       │                                   │ WebSocket: pairing:request        │
       │                                   │ ──────────────────────────────────▶
       │                                   │                                   │
       │   {"status": "pending"}           │            ┌─────────────────────┐│
       │ ◀─────────────────────────────────│            │ 🔔 New Device!      ││
       │                                   │            │ "Robot Alpha"       ││
       │                                   │            │ Code: A7X9K2        ││
       │   User sees code on screen        │            │ Expires: 4:32       ││
       │   Operator tells device user      │            └─────────────────────┘│
       │                                   │                                   │
       │ POST /api/pair/confirm            │                                   │
       │ {device_id, code: "A7X9K2"}       │                                   │
       │ ─────────────────────────────────▶│                                   │
       │                                   │                                   │
       │                                   │ Validate code                     │
       │                                   │ Generate auth token               │
       │                                   │ Store in database                 │
       │                                   │                                   │
       │   {"token": "xyz..."}             │ WebSocket: device:paired          │
       │ ◀─────────────────────────────────│ ──────────────────────────────────▶
       │                                   │                                   │
       │ WebSocket connect with token      │                                   │
       │ ─────────────────────────────────▶│                                   │
       │                                   │                                   │
       │      🎉 DEVICE IS NOW LIVE        │           🎉 SHOWS ON GLOBE       │
```

## Protocol

All communication is JSON over WebSocket.

### Device → Server

```json
// Registration (with auth token)
{"type": "register", "data": {"token": "abc123...", "device_id": "robot-01", "device_type": "robot", "name": "Robot Alpha", "latitude": 34.05, "longitude": -118.24}}

// Telemetry (sent every second)
{"type": "telemetry", "data": {"latitude": 34.05, "longitude": -118.24, "altitude": 0, "heading": 90, "speed": 1.5, "battery": 87, "sensors": {}}}

// Command acknowledgment
{"type": "command:ack", "data": {"commandId": "abc123", "status": "received"}}
```

### Server → Device

```json
// Command
{"type": "command", "data": {"commandId": "abc123", "type": "navigate", "payload": {"latitude": 34.06, "longitude": -118.25}}}
```

### UI ↔ Server

```json
// Request devices
{"type": "getDevices", "data": {}}

// Send command
{"type": "sendCommand", "data": {"deviceId": "robot-01", "commandType": "navigate", "payload": {"latitude": 34.06, "longitude": -118.25}}}

// Receive device list
{"type": "devices:list", "data": [{...}, {...}]}

// Pairing request notification
{"type": "pairing:request", "data": {"device_id": "robot-01", "name": "Robot Alpha", "type": "robot", "code": "A7X9K2", "expires_at": 1234567890}}

// Device paired notification
{"type": "device:paired", "data": {"device_id": "robot-01", "name": "Robot Alpha"}}
```

## Commands

| Command | Payload | Description |
|---------|---------|-------------|
| `navigate` | `{latitude, longitude}` | Move to coordinates |
| `stop` | `{}` | Stop movement |
| `ring` | `{}` | Ring device |
| `photo` | `{}` | Take photo |

## HTTP API

### Pairing

```bash
# Request to join (device calls this)
curl -X POST http://localhost:3000/api/pair/request \
  -H "Content-Type: application/json" \
  -d '{"device_id": "robot-01", "name": "Robot Alpha", "device_type": "robot"}'
# Response: {"status": "pending", "message": "Enter the 6-digit code shown in GlobalUI"}

# Confirm with code (device calls this after user enters code)
curl -X POST http://localhost:3000/api/pair/confirm \
  -H "Content-Type: application/json" \
  -d '{"device_id": "robot-01", "code": "A7X9K2"}'
# Response: {"status": "paired", "token": "abc123..."}
```

### Device Management

```bash
# List all paired devices
curl http://localhost:3000/api/devices

# Revoke a device
curl -X DELETE http://localhost:3000/api/devices/robot-01
```

### Health Data (Oura)

```bash
# Get sleep data (proxied through server to avoid CORS)
curl "http://localhost:3000/api/oura/sleep?start=2024-01-01&end=2024-01-31"
```

## Connecting Devices

### Python (Recommended for Robots)

See `client/device.py` - Zero dependencies, stdlib only.

```bash
cd client
python device.py
# Follow prompts to enter the 6-digit code from GlobalUI
```

### From Scratch (Any Language)

1. POST to `/api/pair/request` with device info
2. User enters 6-digit code from GlobalUI
3. POST to `/api/pair/confirm` with device_id and code
4. Save the returned token
5. Connect WebSocket to `/ws`
6. Send `register` message with token
7. Start telemetry loop

## Accessing From Outside Your Network

### Option A: Port Forwarding (Free)

1. Log into your router admin panel
2. Forward external port 3000 → your machine's local IP:3000
3. Find your public IP (google "what is my ip")
4. Point your domain (e.g., miraeopus.com) A record to your public IP
5. Devices anywhere can connect to `ws://miraeopus.com:3000`

**Gotchas:**
- Your public IP may change (use dynamic DNS)
- Some ISPs block inbound connections
- Some ISPs use CGNAT (shared IP, can't port forward)

### Option B: Run on a Server

If you have a VPS, Raspberry Pi with static IP, or any always-on machine:
1. Copy the binary and `public/` folder
2. Run `./globalrts`
3. Point your domain to that IP

### Option C: SSH Tunnel (For Testing)

```bash
# On your server with public IP
ssh -R 3000:localhost:3000 user@your-server.com
# Now your-server.com:3000 forwards to your local machine
```

## File Structure

```
GlobalRTS/
├── globalrts           # Single binary (~5MB)
├── data/
│   ├── state.db        # SQLite: device registry, pairing, commands
│   └── telemetry/      # JSONL files: time-series data
│       └── YYYY/MM/DD/
│           └── {device}.jsonl
├── public/
│   ├── globalui.html   # Browser interface
│   └── CONFIG.js       # API keys (gitignored)
├── client/
│   └── device.py       # Reference device implementation
└── src/
    ├── main.rs         # Entry point, connection handling
    ├── http.rs         # HTTP server + API endpoints
    ├── websocket.rs    # WebSocket implementation (RFC 6455)
    ├── protocol.rs     # Message types
    ├── state.rs        # SQLite database
    └── telemetry.rs    # JSONL file writer
```

## Roadmap

### Current State (v0.2)

- [x] CesiumJS globe with 3D tiles
- [x] WebSocket server for device registration/telemetry
- [x] Basic command routing (navigate, stop, ring)
- [x] SQLite state storage
- [x] JSONL telemetry logging
- [x] Device pairing with 6-digit codes
- [x] Token-based authentication
- [x] Oura Ring API proxy (health calendar)
- [x] Device management panel
- [x] Python reference client (zero dependencies)

### Phase 2: See and Hear Through Devices

- [ ] MJPEG video streaming (robot → server → browser)
- [ ] Audio streaming (bidirectional)
- [ ] Manual control mode (double-click unit → keyboard control)
- [ ] Server as media proxy

### Phase 3: Scale to 1000 Devices

- [ ] Connection pooling and load testing
- [ ] Selective streaming (only stream active views)
- [ ] UDP telemetry option for high-frequency data (100Hz IMU)
- [ ] WireGuard VPN integration (optional, for private mesh)

### Phase 4: Reprogrammability

- [ ] Device HTTP API (GET /code, POST /code)
- [ ] Hot-reload device code from UI
- [ ] Remote diagnostics and debugging
- [ ] OTA firmware updates

### Phase 5: Intelligence

- [ ] Recording and playback
- [ ] Telemetry visualization/analytics
- [ ] Autonomous behaviors
- [ ] Multi-device coordination

## For Future AIs

If you're an AI reading this to understand or modify GlobalRTS:

1. **Philosophy is paramount.** Every change must maintain: zero runtime dependencies, single binary output, observable/reprogrammable design.

2. **The WebSocket implementation is from scratch.** See `websocket.rs`. Don't add a WebSocket library.

3. **The HTTP server is minimal.** See `http.rs`. It handles static files + API endpoints. Don't add a framework.

4. **Pairing flow is in the database.** Tables: `devices`, `pairing_requests`, `commands`. Tokens are random hex strings.

5. **Video streaming (Phase 2) should use MJPEG.** It's simple, works in `<img>` tags, and doesn't require complex codecs. The server proxies streams from robots to browsers.

6. **Don't add external services.** No Nginx, no Redis, no message queues. If you need a feature, implement it in Rust inside the binary.

7. **The domain (miraeopus.com) is for future use.** Currently running locally. When deployed, it will be the public entry point.

## Building

```bash
# Install Rust (one-time)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build release binary
cargo build --release

# Binary location
./target/release/globalrts
```

## Cross-Compilation

```bash
# For Raspberry Pi
rustup target add arm-unknown-linux-gnueabihf
cargo build --release --target arm-unknown-linux-gnueabihf
```

## License

MIT. Build your robot empire.
