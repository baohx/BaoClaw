#!/usr/bin/env node
/**
 * BaoClaw TUI — 极客禅宗风格终端界面
 * 
 * 设计理念：
 * - 极简：只显示必要信息，去除一切噪音
 * - 禅意：留白即内容，呼吸感节奏
 * - 极客：数据可视化，状态一目了然
 * 
 * 基于 Ink (React for CLI) 构建
 */
import React from 'react';
import { render } from 'ink';
import { App } from './components/App.js';
import { discoverDaemonSocket } from './ipc.js';

async function main() {
  // 发现 daemon socket
  const socketPath = discoverDaemonSocket();
  
  if (!socketPath) {
    console.error('\n  ◉ BaoClaw daemon not running.');
    console.error('\n  Start it with:');
    console.error('    baoclaw\n');
    console.error('  Or specify socket:');
    console.error('    BAOCLAW_SOCKET=/tmp/baoclaw-sockets/xxx.sock npm run tui\n');
    process.exit(1);
  }
  
  // 启动 TUI
  render(<App socketPath={socketPath} />);
}

main().catch((err) => {
  console.error('Failed to start TUI:', err);
  process.exit(1);
});
