// server.js - Simple proxy server for Oura API
// Run with: node server.js
// Then open http://localhost:3000

const http = require('http');
const https = require('https');
const fs = require('fs');
const path = require('path');

// Load config by reading and parsing the file
let OURA_TOKEN = '';
let CESIUM_TOKEN = '';

try {
    const configContent = fs.readFileSync('./CONFIG.js', 'utf8');
    
    // Extract OURA_API_TOKEN
    const ouraMatch = configContent.match(/OURA_API_TOKEN:\s*['"]([^'"]+)['"]/);
    if (ouraMatch) {
        OURA_TOKEN = ouraMatch[1];
        console.log('✓ Loaded Oura API token');
    } else {
        console.error('✗ Could not find OURA_API_TOKEN in CONFIG.js');
    }
    
    // Extract CESIUM_API_TOKEN
    const cesiumMatch = configContent.match(/CESIUM_API_TOKEN:\s*['"]([^'"]+)['"]/);
    if (cesiumMatch) {
        CESIUM_TOKEN = cesiumMatch[1];
        console.log('✓ Loaded Cesium API token');
    }
    
} catch (e) {
    console.error('ERROR: Could not load CONFIG.js');
    console.error('Make sure CONFIG.js exists with your API tokens');
    console.error(e.message);
    process.exit(1);
}

if (!OURA_TOKEN) {
    console.error('ERROR: No Oura token found. Check your CONFIG.js');
    process.exit(1);
}

const PORT = 3000;

const MIME_TYPES = {
    '.html': 'text/html',
    '.js': 'application/javascript',
    '.css': 'text/css',
    '.json': 'application/json',
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.svg': 'image/svg+xml'
};

const server = http.createServer((req, res) => {
    // CORS headers
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
    res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');
    
    if (req.method === 'OPTIONS') {
        res.writeHead(200);
        res.end();
        return;
    }
    
    // Proxy requests to Oura API
    if (req.url.startsWith('/api/oura/')) {
        const ouraPath = req.url.replace('/api/oura', '');
        const ouraUrl = `https://api.ouraring.com${ouraPath}`;
        
        console.log(`Proxying to: ${ouraUrl}`);
        
        const options = {
            method: 'GET',
            headers: {
                'Authorization': `Bearer ${OURA_TOKEN}`,
                'Accept': 'application/json'
            }
        };
        
        const proxyReq = https.request(ouraUrl, options, (proxyRes) => {
            let data = '';
            proxyRes.on('data', chunk => data += chunk);
            proxyRes.on('end', () => {
                console.log(`Response status: ${proxyRes.statusCode}`);
                res.writeHead(proxyRes.statusCode, {
                    'Content-Type': 'application/json',
                    'Access-Control-Allow-Origin': '*'
                });
                res.end(data);
            });
        });
        
        proxyReq.on('error', (e) => {
            console.error('Proxy error:', e);
            res.writeHead(500);
            res.end(JSON.stringify({ error: e.message }));
        });
        
        proxyReq.end();
        return;
    }
    
    // Serve static files
    let filePath = req.url === '/' ? '/GlobalUI.html' : req.url;
    filePath = path.join(__dirname, filePath);
    
    const ext = path.extname(filePath);
    const contentType = MIME_TYPES[ext] || 'application/octet-stream';
    
    fs.readFile(filePath, (err, content) => {
        if (err) {
            if (err.code === 'ENOENT') {
                console.log(`File not found: ${filePath}`);
                res.writeHead(404);
                res.end('File not found: ' + req.url);
            } else {
                res.writeHead(500);
                res.end('Server error');
            }
            return;
        }
        
        res.writeHead(200, { 'Content-Type': contentType });
        res.end(content);
    });
});

server.listen(PORT, () => {
    console.log(`
╔════════════════════════════════════════════════════╗
║  GlobalUI Server Running                           ║
║  Open: http://localhost:${PORT}                       ║
║                                                    ║
║  Oura API proxy available at /api/oura/*           ║
╚════════════════════════════════════════════════════╝
    `);
});
