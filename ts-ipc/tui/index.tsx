#!/usr/bin/env node
import React from 'react';
import { render } from 'ink';
import { App } from './components/App.js';
import { createIpcConnection } from './ipc.js';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

async function main() {
  // Get socket path from args or default
  const socketPath = process.argv[2] || '/tmp/baoclaw.sock';
  
  // Get model from config
  let model = 'unknown';
  try {
    const configPath = path.join(os.homedir(), '.baoclaw', 'config.json');
    if (fs.existsSync(configPath)) {
      const config = JSON.parse(fs.readFileSync(configPath, 'utf-8'));
      model = config.model || config.defaultModel || 'unknown';
    }
  } catch (err) {
    // Ignore config read errors
  }

  // Connect to backend
  console.log(`Connecting to BaoClaw at ${socketPath}...`);
  
  try {
    const client = await createIpcConnection({ socketPath });
    console.log('Connected!');
    
    // Render TUI
    render(React.createElement(App, { client, model }));
  } catch (err) {
    console.error('Failed to connect:', err);
    console.error('\nMake sure BaoClaw is running and the socket path is correct.');
    console.error('Usage: baoclaw-tui [socket-path]');
    process.exit(1);
  }
}

main();
