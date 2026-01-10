# GlobalRTS

Command center for robot fleets.

## Quick Start

```bash
# Build (requires Rust)
cargo build --release

# Run server
./target/release/globalrts

# Open browser
# http://localhost:3000

# Simulate a robot (in another terminal)
./target/release/simulator robot robot-01 "Robot Alpha"
```

## What This Is

A single binary that:
- Serves a 3D globe interface (CesiumJS)
- Accepts WebSocket connections from devices (robots, phones, drones)
- Routes commands from UI to devices
- Stores device state (SQLite, embedded)
- Stores telemetry (flat files, high-throughput)

## Architecture

```
GlobalRTS/
├── globalrts           # Single binary (~3MB)
├── data/
│   ├── state.db        # SQLite: device registry, commands
│   └── telemetry/      # JSONL files: time-series data
│       └── YYYY/MM/DD/
│           └── {device}.jsonl
└── public/
    └── globalui.html   # Browser interface
```

## Philosophy

See [PHILOSOPHY.md](PHILOSOPHY.md) for the full manifesto.

**TL;DR:**
- Observable: Every byte of data inspectable
- Reprogrammable: Change anything remotely
- 1000-year-proof: No external dependencies that can break
- Just run it: Single binary, no setup

## Building

```bash
# Install Rust (one-time)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build release binary
cargo build --release

# Binary location
./target/release/globalrts
./target/release/simulator
```

## Cross-Compilation

```bash
# For Raspberry Pi
rustup target add arm-unknown-linux-gnueabihf
cargo build --release --target arm-unknown-linux-gnueabihf

# For other targets, see: rustup target list
```

## Configuration

Edit constants in `src/main.rs`:

```rust
const PORT: u16 = 3000;
const PUBLIC_DIR: &str = "public";
const DATA_DIR: &str = "data";
```

No config files. No environment variables. Change the code, rebuild.

## Protocol

All communication is JSON over WebSocket.

### Device → Server

```json
// Register
{"type": "register", "data": {"device_id": "robot-01", "device_type": "robot", "name": "Robot Alpha", "latitude": 34.05, "longitude": -118.24}}

// Telemetry (sent every second)
{"type": "telemetry", "data": {"latitude": 34.05, "longitude": -118.24, "altitude": 0, "heading": 90, "speed": 1.5, "battery": 87}}

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

// Receive updates
{"type": "device:update", "data": {...}}
{"type": "device:online", "data": {...}}
{"type": "device:offline", "data": {"deviceId": "robot-01"}}
```

## Commands

| Command | Payload | Description |
|---------|---------|-------------|
| `navigate` | `{latitude, longitude}` | Move to coordinates |
| `stop` | `{}` | Stop movement |
| `ring` | `{}` | Ring device |
| `photo` | `{}` | Take photo |

## Data Storage

### State (SQLite)

Current device state, command history. Small, transactional.

```sql
SELECT * FROM devices;
SELECT * FROM commands WHERE device_id = 'robot-01';
```

### Telemetry (JSONL Files)

High-volume time-series. One file per device per day.

```
data/telemetry/2026/01/10/robot-01.jsonl
```

Each line:
```json
{"timestamp":1704844800,"device_id":"robot-01","latitude":34.05,"longitude":-118.24,"altitude":0,"heading":90,"speed":1.5,"battery":87,"sensors":{}}
```

For analysis, use DuckDB:
```sql
SELECT * FROM 'data/telemetry/2026/01/*.jsonl' WHERE battery < 20;
```

## Integrating Real Devices

### Python

```python
import websocket
import json

ws = websocket.create_connection("ws://localhost:3000")

# Register
ws.send(json.dumps({
    "type": "register",
    "data": {
        "device_id": "robot-01",
        "device_type": "robot", 
        "name": "My Robot",
        "latitude": 34.05,
        "longitude": -118.24
    }
}))

# Send telemetry loop
while True:
    ws.send(json.dumps({
        "type": "telemetry",
        "data": {
            "latitude": get_gps_lat(),
            "longitude": get_gps_lon(),
            "battery": get_battery()
        }
    }))
    time.sleep(1)
```

### C/C++ (for embedded)

Use any WebSocket library, or implement RFC 6455 directly (~300 lines).

## License

MIT. Build your robot empire.
