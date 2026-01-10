# GlobalRTS - Command Center

Self-hosted real-time strategy interface for controlling robots, phones, and IoT devices on a 3D globe.

## Features

- 🌍 **3D Globe Interface** - CesiumJS with Google Photorealistic 3D Tiles
- 🤖 **Device Control** - Command robots, phones, IoT devices in real-time
- 📍 **Live Tracking** - See all devices on the map with live position updates
- 🎮 **RTS Controls** - Click to select, right-click to set waypoints, drag to box-select
- 📰 **Global News Feed** - 8-column expandable news from every continent
- 💚 **Health Dashboard** - Oura Ring integration for sleep/health data
- 🔌 **WebSocket** - Real-time bidirectional communication

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                         GlobalUI                                │
│              (CesiumJS 3D Globe - Your Browser)                 │
│                                                                 │
│   • See all devices on map in real-time                        │
│   • Click to select, view telemetry                            │
│   • Right-click to send navigation commands                    │
│   • Drag to box-select multiple units                          │
└──────────────────────────┬─────────────────────────────────────┘
                           │ Socket.IO (WebSocket)
                           ▼
┌────────────────────────────────────────────────────────────────┐
│                   Command Center Server                         │
│                     (Node.js + Express)                         │
│                                                                 │
│   • REST API for device management                             │
│   • WebSocket for real-time communication                      │
│   • PostgreSQL for persistence                                  │
│   • Routes commands to devices                                  │
└──────────────────────────┬─────────────────────────────────────┘
                           │
          ┌────────────────┼────────────────┐
          ▼                ▼                ▼
     ┌─────────┐     ┌──────────┐     ┌─────────┐
     │ iPhone  │     │  Robot   │     │   IoT   │
     │   App   │     │  Swarm   │     │ Devices │
     └─────────┘     └──────────┘     └─────────┘
```

## Quick Start

### 1. Install Node.js
Download from https://nodejs.org/ (LTS version recommended)

### 2. Setup

```bash
# Navigate to GlobalRTS folder
cd GlobalRTS

# Install dependencies
npm install

# Copy config templates
cp .env.example .env
cp public/CONFIG.template.js public/CONFIG.js

# Edit public/CONFIG.js with your Cesium API token
# Get one free at: https://cesium.com/ion/tokens
```

### 3. Start Server

```bash
npm start
```

### 4. Open GlobalUI

Open browser to: **http://localhost:3000/globalui.html**

### 5. Test with Simulator

In a new terminal:

```bash
# Simulate a robot
node simulator.js robot robot-01 "Robot Alpha"

# Simulate your phone
node simulator.js phone my-iphone "Jonathan's iPhone"

# Simulate an IoT sensor
node simulator.js iot garage-sensor "Garage Sensor"
```

Watch the devices appear on your globe!

## File Structure

```
GlobalRTS/
├── server.js              # Main server (REST + WebSocket)
├── simulator.js           # Test device simulator
├── package.json           # Node.js dependencies
├── docker-compose.yml     # Docker deployment
├── Dockerfile
├── .env.example           # Server config template
├── .gitignore             # Git ignore rules
└── public/
    ├── globalui.html      # Main interface
    ├── CONFIG.template.js # API keys template
    └── CONFIG.js          # Your API keys (gitignored)
```

## Configuration

### API Keys (public/CONFIG.js)

```javascript
const CONFIG = {
    CESIUM_API_TOKEN: 'your-cesium-token',
    OURA_API_TOKEN: 'your-oura-token'  // Optional
};
```

### Server (.env)

```bash
PORT=3000
DB_HOST=localhost
DB_PORT=5432
DB_NAME=command_center
DB_USER=commander
DB_PASSWORD=localdev123
```

## Docker Deployment (Optional)

```bash
# Start server + PostgreSQL
docker-compose up -d

# View logs
docker-compose logs -f

# Stop
docker-compose down
```

## Commands

Send commands to devices via the GlobalUI or REST API:

| Command | Description | Payload |
|---------|-------------|---------|
| `navigate` | Move to coordinates | `{ latitude, longitude }` |
| `stop` | Stop movement | `{}` |
| `returnHome` | Return to home position | `{}` |
| `ring` | Ring phone | `{}` |
| `photo` | Take photo | `{}` |

## REST API

```bash
# Get all devices
GET /api/devices

# Send command
POST /api/devices/:id/command
{ "commandType": "navigate", "payload": { "latitude": 34.05, "longitude": -118.25 } }

# Health check
GET /api/health
```

## WebSocket Events

### Device → Server
- `register` - Register device
- `telemetry` - Send position/sensor data
- `command:ack` - Acknowledge command
- `command:complete` - Command finished

### Server → Device
- `command` - Execute command

### GlobalUI → Server
- `getDevices` - Request device list
- `sendCommand` - Send command to device

## Integrating Real Devices

### iOS App
Use Socket.IO Swift client to:
1. Connect to server
2. Register as device
3. Send GPS updates
4. Listen for commands

### Robots
Connect via Socket.IO or MQTT:
1. Register on connect
2. Stream telemetry (GPS, battery, sensors)
3. Execute navigation commands
4. Report completion

## Troubleshooting

### "Server: Connection failed"
- Make sure server is running: `npm start`
- Check if port 3000 is available

### Devices not showing
- Check browser console for errors
- Verify Cesium token in CONFIG.js
- Make sure simulator is connected

### Database errors
- Server works without PostgreSQL (in-memory mode)
- For persistence, install PostgreSQL or use Docker

## License

MIT
