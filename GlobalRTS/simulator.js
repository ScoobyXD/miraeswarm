/**
 * DEVICE SIMULATOR
 * 
 * Simulates phones, robots, and IoT devices connecting to the Command Center.
 * Run this to test the system without real hardware.
 * 
 * Usage: node simulator.js [device-type] [device-id]
 * Examples:
 *   node simulator.js robot robot-01
 *   node simulator.js phone my-iphone
 *   node simulator.js iot sensor-garage
 */

const io = require('socket.io-client');

// Configuration
const SERVER_URL = process.env.SERVER_URL || 'http://localhost:3000';
const DEVICE_TYPE = process.argv[2] || 'robot';
const DEVICE_ID = process.argv[3] || `${DEVICE_TYPE}-${Date.now().toString(36)}`;
const DEVICE_NAME = process.argv[4] || `Simulated ${DEVICE_TYPE.charAt(0).toUpperCase() + DEVICE_TYPE.slice(1)}`;

// Starting position (Downtown LA)
let position = {
    latitude: 34.0522 + (Math.random() - 0.5) * 0.01,
    longitude: -118.2437 + (Math.random() - 0.5) * 0.01,
    altitude: 0,
    heading: Math.random() * 360,
    speed: 0
};

let battery = 85 + Math.random() * 15;
let status = 'idle';
let targetPosition = null;

console.log('\n========================================');
console.log('   DEVICE SIMULATOR');
console.log('========================================');
console.log(`Type: ${DEVICE_TYPE}`);
console.log(`ID: ${DEVICE_ID}`);
console.log(`Name: ${DEVICE_NAME}`);
console.log(`Server: ${SERVER_URL}`);
console.log('========================================\n');

// Connect to server
const socket = io(SERVER_URL);

socket.on('connect', () => {
    console.log('✓ Connected to Command Center');
    
    // Register this device
    socket.emit('register', {
        deviceId: DEVICE_ID,
        deviceType: DEVICE_TYPE,
        name: DEVICE_NAME,
        latitude: position.latitude,
        longitude: position.longitude,
        metadata: {
            simulator: true,
            version: '1.0.0'
        }
    });
});

socket.on('registered', (data) => {
    console.log('✓ Device registered:', data.device.name);
    console.log(`  Position: ${position.latitude.toFixed(6)}, ${position.longitude.toFixed(6)}`);
    
    // Start sending telemetry
    startTelemetry();
});

socket.on('disconnect', () => {
    console.log('✗ Disconnected from server');
});

socket.on('connect_error', (err) => {
    console.log('✗ Connection error:', err.message);
});

// Handle commands from server
socket.on('command', (cmd) => {
    console.log(`\n📥 Command received: ${cmd.type}`);
    console.log('   Payload:', JSON.stringify(cmd.payload));
    
    // Acknowledge command
    socket.emit('command:ack', {
        commandId: cmd.commandId,
        status: 'received'
    });
    
    // Execute command
    executeCommand(cmd);
});

function executeCommand(cmd) {
    switch (cmd.type) {
        case 'navigate':
            // Set target and start moving
            targetPosition = {
                latitude: cmd.payload.latitude,
                longitude: cmd.payload.longitude
            };
            status = 'moving';
            console.log(`   🚀 Navigating to: ${targetPosition.latitude.toFixed(6)}, ${targetPosition.longitude.toFixed(6)}`);
            
            socket.emit('command:ack', {
                commandId: cmd.commandId,
                status: 'executing'
            });
            break;
            
        case 'stop':
            targetPosition = null;
            status = 'idle';
            position.speed = 0;
            console.log('   🛑 Stopped');
            
            socket.emit('command:complete', {
                commandId: cmd.commandId,
                result: { stopped: true }
            });
            break;
            
        case 'ring':
            // Simulate phone ringing
            console.log('   🔔 RING RING RING!');
            status = 'ringing';
            
            setTimeout(() => {
                status = 'idle';
                socket.emit('command:complete', {
                    commandId: cmd.commandId,
                    result: { rang: true, duration: 5 }
                });
            }, 5000);
            break;
            
        case 'photo':
            // Simulate taking photo
            console.log('   📷 Taking photo...');
            
            setTimeout(() => {
                socket.emit('command:complete', {
                    commandId: cmd.commandId,
                    result: { 
                        photo: 'simulated-photo-data',
                        timestamp: new Date().toISOString()
                    }
                });
            }, 1000);
            break;
            
        case 'returnHome':
            targetPosition = {
                latitude: 34.0522,
                longitude: -118.2437
            };
            status = 'returning';
            console.log('   🏠 Returning home');
            break;
            
        default:
            console.log(`   ❓ Unknown command: ${cmd.type}`);
            socket.emit('command:complete', {
                commandId: cmd.commandId,
                result: { error: 'Unknown command' }
            });
    }
}

// Simulate movement
function updatePosition() {
    if (targetPosition && (status === 'moving' || status === 'returning')) {
        const dlat = targetPosition.latitude - position.latitude;
        const dlon = targetPosition.longitude - position.longitude;
        const distance = Math.sqrt(dlat * dlat + dlon * dlon);
        
        if (distance < 0.0001) {
            // Arrived at target
            position.latitude = targetPosition.latitude;
            position.longitude = targetPosition.longitude;
            position.speed = 0;
            status = 'idle';
            targetPosition = null;
            console.log('   ✓ Arrived at destination');
        } else {
            // Move towards target
            const speed = 0.0002; // degrees per tick
            position.latitude += (dlat / distance) * speed;
            position.longitude += (dlon / distance) * speed;
            position.heading = Math.atan2(dlon, dlat) * 180 / Math.PI;
            position.speed = speed * 111000; // Convert to m/s approximately
        }
    }
    
    // Simulate battery drain
    battery = Math.max(0, battery - 0.001);
    
    // Add some random noise to simulate GPS jitter
    if (DEVICE_TYPE === 'phone') {
        position.latitude += (Math.random() - 0.5) * 0.00001;
        position.longitude += (Math.random() - 0.5) * 0.00001;
    }
}

// Send telemetry periodically
function startTelemetry() {
    setInterval(() => {
        updatePosition();
        
        const telemetry = {
            latitude: position.latitude,
            longitude: position.longitude,
            altitude: position.altitude,
            heading: position.heading,
            speed: position.speed,
            battery: battery,
            sensors: {
                temperature: 22 + Math.random() * 5,
                humidity: 45 + Math.random() * 20
            }
        };
        
        socket.emit('telemetry', telemetry);
        
        // Log status every 10 seconds
        if (Date.now() % 10000 < 1000) {
            console.log(`📍 ${position.latitude.toFixed(6)}, ${position.longitude.toFixed(6)} | 🔋 ${battery.toFixed(1)}% | Status: ${status}`);
        }
    }, 1000);
}

// Handle graceful shutdown
process.on('SIGINT', () => {
    console.log('\n\nShutting down simulator...');
    socket.disconnect();
    process.exit(0);
});

console.log('Connecting to server...');
