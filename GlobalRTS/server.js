/**
 * GlobalRTS SERVER
 * 
 * Central hub for controlling all devices:
 * - Robots, phones, IoT devices
 * - Real-time bidirectional communication
 * - PostgreSQL database for persistence
 * - REST API + WebSocket (Socket.IO)
 * 
 * Created by Jonathan Kim
 */

require('dotenv').config();

const express = require('express');
const http = require('http');
const { Server } = require('socket.io');
const cors = require('cors');
const { Pool } = require('pg');
const path = require('path');

// ============================================
// CONFIGURATION
// ============================================
const CONFIG = {
    PORT: process.env.PORT || 3000,
    DB: {
        host: process.env.DB_HOST || 'localhost',
        port: process.env.DB_PORT || 5432,
        database: process.env.DB_NAME || 'globalrts',
        user: process.env.DB_USER || 'jonathan',
        password: process.env.DB_PASSWORD || 'localdev123'
    }
};

// ============================================
// EXPRESS + SOCKET.IO SETUP
// ============================================
const app = express();
const server = http.createServer(app);
const io = new Server(server, {
    cors: {
        origin: "*",
        methods: ["GET", "POST"]
    }
});

app.use(cors());
app.use(express.json());
app.use(express.static(path.join(__dirname, 'public')));

// ============================================
// DATABASE CONNECTION
// ============================================
const pool = new Pool(CONFIG.DB);

// Test database connection
async function testDB() {
    try {
        const client = await pool.connect();
        console.log('✓ PostgreSQL connected');
        client.release();
        return true;
    } catch (err) {
        console.log('✗ PostgreSQL connection failed:', err.message);
        console.log('  Server will run without database persistence');
        return false;
    }
}

// ============================================
// IN-MEMORY STATE (fallback if no DB)
// ============================================
const state = {
    devices: new Map(),      // deviceId -> device info
    connections: new Map(),  // socketId -> deviceId
    commandQueue: []         // pending commands
};

// ============================================
// DATABASE INITIALIZATION
// ============================================
async function initDatabase() {
    const client = await pool.connect();
    try {
        await client.query(`
            -- Devices table
            CREATE TABLE IF NOT EXISTS devices (
                id VARCHAR(64) PRIMARY KEY,
                name VARCHAR(128),
                type VARCHAR(32) NOT NULL,  -- 'robot', 'phone', 'iot'
                status VARCHAR(32) DEFAULT 'offline',
                latitude DOUBLE PRECISION,
                longitude DOUBLE PRECISION,
                altitude DOUBLE PRECISION,
                heading DOUBLE PRECISION,
                speed DOUBLE PRECISION,
                battery DOUBLE PRECISION,
                metadata JSONB DEFAULT '{}',
                last_seen TIMESTAMP DEFAULT NOW(),
                created_at TIMESTAMP DEFAULT NOW()
            );

            -- Telemetry history
            CREATE TABLE IF NOT EXISTS telemetry (
                id SERIAL PRIMARY KEY,
                device_id VARCHAR(64) REFERENCES devices(id),
                latitude DOUBLE PRECISION,
                longitude DOUBLE PRECISION,
                altitude DOUBLE PRECISION,
                heading DOUBLE PRECISION,
                speed DOUBLE PRECISION,
                battery DOUBLE PRECISION,
                sensors JSONB DEFAULT '{}',
                timestamp TIMESTAMP DEFAULT NOW()
            );

            -- Command history
            CREATE TABLE IF NOT EXISTS commands (
                id SERIAL PRIMARY KEY,
                device_id VARCHAR(64) REFERENCES devices(id),
                command_type VARCHAR(64) NOT NULL,
                payload JSONB DEFAULT '{}',
                status VARCHAR(32) DEFAULT 'pending',
                sent_at TIMESTAMP DEFAULT NOW(),
                acknowledged_at TIMESTAMP,
                completed_at TIMESTAMP
            );

            -- Create indexes for performance
            CREATE INDEX IF NOT EXISTS idx_telemetry_device ON telemetry(device_id);
            CREATE INDEX IF NOT EXISTS idx_telemetry_time ON telemetry(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_commands_device ON commands(device_id);
            CREATE INDEX IF NOT EXISTS idx_devices_type ON devices(type);
        `);
        console.log('✓ Database tables initialized');
    } finally {
        client.release();
    }
}

// ============================================
// DEVICE MANAGEMENT
// ============================================

// Register or update a device
async function registerDevice(deviceData) {
    const { id, name, type, latitude, longitude, metadata } = deviceData;
    
    // Update in-memory state
    state.devices.set(id, {
        ...deviceData,
        status: 'online',
        lastSeen: new Date()
    });
    
    // Try to persist to database
    try {
        await pool.query(`
            INSERT INTO devices (id, name, type, latitude, longitude, metadata, status, last_seen)
            VALUES ($1, $2, $3, $4, $5, $6, 'online', NOW())
            ON CONFLICT (id) DO UPDATE SET
                name = COALESCE($2, devices.name),
                type = $3,
                latitude = COALESCE($4, devices.latitude),
                longitude = COALESCE($5, devices.longitude),
                metadata = devices.metadata || COALESCE($6, '{}'),
                status = 'online',
                last_seen = NOW()
        `, [id, name, type, latitude, longitude, JSON.stringify(metadata || {})]);
    } catch (err) {
        console.log('DB write failed (device):', err.message);
    }
    
    return state.devices.get(id);
}

// Update device telemetry
async function updateTelemetry(deviceId, telemetry) {
    const device = state.devices.get(deviceId);
    if (!device) return null;
    
    // Update in-memory
    Object.assign(device, telemetry, { lastSeen: new Date(), status: 'online' });
    
    // Persist to database
    try {
        const { latitude, longitude, altitude, heading, speed, battery, sensors } = telemetry;
        
        // Update device current state
        await pool.query(`
            UPDATE devices SET
                latitude = COALESCE($2, latitude),
                longitude = COALESCE($3, longitude),
                altitude = COALESCE($4, altitude),
                heading = COALESCE($5, heading),
                speed = COALESCE($6, speed),
                battery = COALESCE($7, battery),
                status = 'online',
                last_seen = NOW()
            WHERE id = $1
        `, [deviceId, latitude, longitude, altitude, heading, speed, battery]);
        
        // Log telemetry history (sample every 5 seconds to avoid bloat)
        await pool.query(`
            INSERT INTO telemetry (device_id, latitude, longitude, altitude, heading, speed, battery, sensors)
            SELECT $1, $2, $3, $4, $5, $6, $7, $8
            WHERE NOT EXISTS (
                SELECT 1 FROM telemetry 
                WHERE device_id = $1 
                AND timestamp > NOW() - INTERVAL '5 seconds'
            )
        `, [deviceId, latitude, longitude, altitude, heading, speed, battery, JSON.stringify(sensors || {})]);
    } catch (err) {
        console.log('DB write failed (telemetry):', err.message);
    }
    
    return device;
}

// Send command to device
async function sendCommand(deviceId, commandType, payload = {}) {
    const command = {
        id: Date.now().toString(36) + Math.random().toString(36).substr(2),
        deviceId,
        type: commandType,
        payload,
        status: 'pending',
        sentAt: new Date()
    };
    
    // Find device's socket and send
    for (const [socketId, dId] of state.connections) {
        if (dId === deviceId) {
            io.to(socketId).emit('command', {
                commandId: command.id,
                type: commandType,
                payload
            });
            command.status = 'sent';
            break;
        }
    }
    
    // Persist to database
    try {
        await pool.query(`
            INSERT INTO commands (device_id, command_type, payload, status, sent_at)
            VALUES ($1, $2, $3, $4, NOW())
        `, [deviceId, commandType, JSON.stringify(payload), command.status]);
    } catch (err) {
        console.log('DB write failed (command):', err.message);
    }
    
    return command;
}

// ============================================
// SOCKET.IO - REAL-TIME COMMUNICATION
// ============================================
io.on('connection', (socket) => {
    console.log(`Socket connected: ${socket.id}`);
    
    // ---------- DEVICE EVENTS ----------
    
    // Device registration (phone, robot, IoT connects)
    socket.on('register', async (data) => {
        const { deviceId, deviceType, name, latitude, longitude, metadata } = data;
        
        state.connections.set(socket.id, deviceId);
        
        const device = await registerDevice({
            id: deviceId,
            name: name || `${deviceType}-${deviceId.substr(0, 6)}`,
            type: deviceType,
            latitude,
            longitude,
            metadata
        });
        
        console.log(`✓ Device registered: ${device.name} (${deviceType})`);
        
        // Notify all GlobalUI clients
        io.emit('device:online', device);
        
        // Send ack to device
        socket.emit('registered', { success: true, device });
    });
    
    // Device sends telemetry update
    socket.on('telemetry', async (data) => {
        const deviceId = state.connections.get(socket.id);
        if (!deviceId) return;
        
        const device = await updateTelemetry(deviceId, data);
        
        // Broadcast to all GlobalUI clients
        if (device) {
            io.emit('device:update', device);
        }
    });
    
    // Device acknowledges command
    socket.on('command:ack', async (data) => {
        const { commandId, status } = data;
        console.log(`Command ${commandId} acknowledged: ${status}`);
        
        // Update command status in DB
        try {
            await pool.query(`
                UPDATE commands SET 
                    status = $2,
                    acknowledged_at = NOW()
                WHERE id = $1 OR id::text LIKE $1 || '%'
            `, [commandId, status]);
        } catch (err) {}
        
        // Notify GlobalUI
        io.emit('command:status', { commandId, status });
    });
    
    // Device completes command
    socket.on('command:complete', async (data) => {
        const { commandId, result } = data;
        console.log(`Command ${commandId} completed`);
        
        try {
            await pool.query(`
                UPDATE commands SET 
                    status = 'completed',
                    completed_at = NOW()
                WHERE id = $1 OR id::text LIKE $1 || '%'
            `, [commandId]);
        } catch (err) {}
        
        io.emit('command:complete', { commandId, result });
    });
    
    // ---------- GLOBALUI EVENTS ----------
    
    // GlobalUI requests all devices
    socket.on('getDevices', async () => {
        let devices = [];
        
        try {
            const result = await pool.query('SELECT * FROM devices ORDER BY last_seen DESC');
            devices = result.rows;
        } catch (err) {
            // Fallback to in-memory
            devices = Array.from(state.devices.values());
        }
        
        socket.emit('devices:list', devices);
    });
    
    // GlobalUI sends command to device
    socket.on('sendCommand', async (data) => {
        const { deviceId, commandType, payload } = data;
        
        console.log(`Command: ${commandType} -> ${deviceId}`);
        const command = await sendCommand(deviceId, commandType, payload);
        
        socket.emit('command:sent', command);
    });
    
    // GlobalUI requests device history
    socket.on('getHistory', async (data) => {
        const { deviceId, hours = 24 } = data;
        
        try {
            const result = await pool.query(`
                SELECT * FROM telemetry 
                WHERE device_id = $1 
                AND timestamp > NOW() - INTERVAL '${hours} hours'
                ORDER BY timestamp ASC
            `, [deviceId]);
            
            socket.emit('device:history', { deviceId, history: result.rows });
        } catch (err) {
            socket.emit('device:history', { deviceId, history: [] });
        }
    });
    
    // ---------- DISCONNECT ----------
    
    socket.on('disconnect', async () => {
        const deviceId = state.connections.get(socket.id);
        
        if (deviceId) {
            const device = state.devices.get(deviceId);
            if (device) {
                device.status = 'offline';
                
                try {
                    await pool.query(`
                        UPDATE devices SET status = 'offline' WHERE id = $1
                    `, [deviceId]);
                } catch (err) {}
                
                io.emit('device:offline', { deviceId });
                console.log(`✗ Device disconnected: ${device.name}`);
            }
            
            state.connections.delete(socket.id);
        }
        
        console.log(`Socket disconnected: ${socket.id}`);
    });
});

// ============================================
// REST API ENDPOINTS
// ============================================

// API root - list available endpoints
app.get('/api', (req, res) => {
    res.json({
        name: 'GlobalRTS API',
        version: '1.0.0',
        endpoints: {
            health: 'GET /api/health',
            devices: 'GET /api/devices',
            device: 'GET /api/devices/:id',
            sendCommand: 'POST /api/devices/:id/command',
            history: 'GET /api/devices/:id/history',
            commands: 'GET /api/devices/:id/commands'
        }
    });
});

// Health check
app.get('/api/health', (req, res) => {
    res.json({ 
        status: 'ok', 
        uptime: process.uptime(),
        devices: state.devices.size,
        connections: state.connections.size
    });
});

// Get all devices
app.get('/api/devices', async (req, res) => {
    try {
        const result = await pool.query('SELECT * FROM devices ORDER BY last_seen DESC');
        res.json(result.rows);
    } catch (err) {
        res.json(Array.from(state.devices.values()));
    }
});

// Get single device
app.get('/api/devices/:id', async (req, res) => {
    try {
        const result = await pool.query('SELECT * FROM devices WHERE id = $1', [req.params.id]);
        if (result.rows.length === 0) {
            return res.status(404).json({ error: 'Device not found' });
        }
        res.json(result.rows[0]);
    } catch (err) {
        const device = state.devices.get(req.params.id);
        if (!device) return res.status(404).json({ error: 'Device not found' });
        res.json(device);
    }
});

// Register device via REST
app.post('/api/devices', async (req, res) => {
    const device = await registerDevice(req.body);
    io.emit('device:online', device);
    res.json(device);
});

// Send command via REST
app.post('/api/devices/:id/command', async (req, res) => {
    const { commandType, payload } = req.body;
    const command = await sendCommand(req.params.id, commandType, payload);
    res.json(command);
});

// Get device telemetry history
app.get('/api/devices/:id/history', async (req, res) => {
    const hours = parseInt(req.query.hours) || 24;
    
    try {
        const result = await pool.query(`
            SELECT * FROM telemetry 
            WHERE device_id = $1 
            AND timestamp > NOW() - INTERVAL '${hours} hours'
            ORDER BY timestamp ASC
        `, [req.params.id]);
        res.json(result.rows);
    } catch (err) {
        res.json([]);
    }
});

// Get all commands for a device
app.get('/api/devices/:id/commands', async (req, res) => {
    try {
        const result = await pool.query(`
            SELECT * FROM commands 
            WHERE device_id = $1 
            ORDER BY sent_at DESC 
            LIMIT 100
        `, [req.params.id]);
        res.json(result.rows);
    } catch (err) {
        res.json([]);
    }
});

// ============================================
// VIDEO STREAMING ENDPOINT (placeholder)
// ============================================
app.get('/api/devices/:id/stream', (req, res) => {
    // This would be implemented with WebRTC or RTSP relay
    // For now, return info about how to connect
    res.json({
        deviceId: req.params.id,
        streamType: 'webrtc',
        message: 'Video streaming requires WebRTC signaling - use Socket.IO events'
    });
});

// ============================================
// OURA API PROXY
// ============================================
const https = require('https');

app.get('/api/oura/*', (req, res) => {
    const ouraPath = req.params[0];
    const token = process.env.OURA_API_TOKEN;
    
    if (!token) {
        return res.status(401).json({ error: 'No Oura token in .env file' });
    }
    
    const queryString = req.url.includes('?') ? req.url.substring(req.url.indexOf('?')) : '';
    
    const options = {
        hostname: 'api.ouraring.com',
        path: '/' + ouraPath + queryString,
        headers: { 'Authorization': `Bearer ${token}` }
    };
    
    https.get(options, (apiRes) => {
        let data = '';
        apiRes.on('data', chunk => data += chunk);
        apiRes.on('end', () => {
            try {
                res.status(apiRes.statusCode).json(JSON.parse(data));
            } catch (e) {
                res.status(500).json({ error: 'Invalid response from Oura API' });
            }
        });
    }).on('error', (err) => {
        res.status(500).json({ error: err.message });
    });
});

// ============================================
// START SERVER
// ============================================
async function start() {
    console.log('\n========================================');
    console.log('   GlobalRTS SERVER');
    console.log('========================================\n');
    
    const dbConnected = await testDB();
    
    if (dbConnected) {
        try {
            await initDatabase();
        } catch (err) {
            console.log('Database init error:', err.message);
        }
    }
    
    server.listen(CONFIG.PORT, () => {
        console.log(`✓ Server running on http://localhost:${CONFIG.PORT}`);
        console.log(`\n  GlobalUI:    http://localhost:${CONFIG.PORT}/globalui.html`);
        console.log(`  REST API:    http://localhost:${CONFIG.PORT}/api`);
        console.log(`  WebSocket:   ws://localhost:${CONFIG.PORT}`);
        console.log('\n========================================\n');
    });
}

start();
