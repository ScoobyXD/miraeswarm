# GlobalRTS Device Client

Zero-dependency Python client for connecting devices to GlobalRTS.

## Requirements

- Python 3.6+
- No external packages (uses stdlib only)

## Quick Start

```bash
python device.py
```

On first run:
1. Script requests pairing with server
2. Look at GlobalUI for 6-digit code
3. Enter code when prompted
4. Token is saved to `device_token.txt`
5. Device connects and starts sending telemetry

Subsequent runs use saved token automatically.

## Options

```bash
python device.py --help

Options:
  --server HOST:PORT   Server address (default: localhost:3000)
  --id DEVICE_ID       Device ID (auto-generated if not set)
  --name NAME          Device name (default: "Python Device")
  --type TYPE          Device type: robot, phone, drone, etc.
  --reset              Clear saved token and re-pair
```

## Examples

```bash
# Connect as a robot
python device.py --name "My Robot" --type robot

# Connect to remote server
python device.py --server miraeopus.com:3000

# Force re-pairing
python device.py --reset
```

## How It Works

1. **Pairing Flow**
   - POST `/api/pair/request` with device info
   - Server shows 6-digit code in GlobalUI
   - You enter code
   - POST `/api/pair/confirm` to get auth token

2. **WebSocket Connection**
   - Connect to `ws://server:port/`
   - Send `register` message with token
   - Start telemetry loop

3. **Telemetry**
   - Sends position, heading, speed, battery every second
   - Currently simulates movement (random walk)
   - Replace `DeviceState` class with real sensor readings

4. **Commands**
   - Receives commands from GlobalUI
   - Implements: `navigate`, `stop`, `ring`
   - Add your own command handlers

## Integration

To use in your own robot code:

```python
from device import WebSocketClient, pair_device, load_token, save_token

# Check for saved token
token, device_id = load_token()

if not token:
    # Pair with server
    token = pair_device("localhost:3000", "my-robot", "Robot", "robot")
    save_token(token, "my-robot")

# Connect
ws = WebSocketClient("ws://localhost:3000/")
ws.connect()

# Register
ws.send({
    "type": "register",
    "data": {
        "token": token,
        "device_id": "my-robot",
        "device_type": "robot",
        "name": "Robot",
        "latitude": 34.0522,
        "longitude": -118.2437
    }
})

# Send telemetry
while True:
    ws.send({
        "type": "telemetry",
        "data": {
            "latitude": get_gps_lat(),
            "longitude": get_gps_lon(),
            "battery": get_battery_percent(),
            # ...
        }
    })
    time.sleep(1)
```

## Files

- `device.py` - Main client script
- `device_token.txt` - Saved auth token (created after pairing)
