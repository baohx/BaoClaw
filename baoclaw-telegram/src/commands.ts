/**
 * Command module for BaoClaw Telegram Gateway.
 * Contains RPC response types, command registry, and parsing utilities.
 * Format functions and handlers are added in later tasks.
 */

// ═══════════════════════════════════════════════════════════════
// RPC Response Type Interfaces
// ═══════════════════════════════════════════════════════════════

export interface ToolInfo {
  name: string;
  description: string;
  type: string; // 'builtin' | 'mcp' | 'plugin'
}

export interface SkillInfo {
  name: string;
  path: string;
  source: string; // 'project' | 'global'
  description?: string;
}

export interface McpServerInfo {
  name: string;
  server_type: string; // 'stdio' | 'sse'
  disabled: boolean;
  source: string;
  command?: string;
  url?: string;
  config_path: string;
}

export interface PluginInfo {
  name: string;
  version?: string;
  description?: string;
  path: string;
  source: string;
  has_tools: boolean;
  has_skills: boolean;
  has_mcp: boolean;
}

export interface CompactResult {
  tokens_saved: number;
  summary_tokens: number;
  tokens_before: number;
  tokens_after: number;
}

export interface GitStatusResult {
  branch: string | null;
  has_changes: boolean;
  staged_files: string[];
  modified_files: string[];
  untracked_files: string[];
}

export interface GitCommitResult {
  hash: string;
  message: string;
}

export interface GitDiffResult {
  diff: string;
}

export interface InitializeResult {
  capabilities: { tools: boolean; streaming: boolean; permissions: boolean };
  session_id: string;
  reconnected: boolean;
  resumed: boolean;
  message_count: number;
  shared?: boolean;
}

export interface SessionState {
  resumed: boolean;
  messageCount: number;
  sessionId: string;
  shared?: boolean;
}

// ═══════════════════════════════════════════════════════════════
// Command Definition & Registry
// ═══════════════════════════════════════════════════════════════

export interface CommandDefinition {
  description: string;
}

export const COMMAND_REGISTRY: Record<string, CommandDefinition> = {
  '/tools':   { description: '列出已注册的工具' },
  '/skills':  { description: '列出已加载的技能' },
  '/mcp':     { description: '列出 MCP 服务器' },
  '/plugins': { description: '列出已安装的插件' },
  '/compact': { description: '压缩对话上下文' },
  '/think':   { description: '切换扩展思考模式' },
  '/model':   { description: '查看或切换模型' },
  '/diff':    { description: '查看 git diff' },
  '/commit':  { description: '提交 git 变更' },
  '/git':     { description: '查看 git 状态' },
  '/abort':   { description: '中止当前任务' },
  '/help':    { description: '显示帮助信息' },
  '/status':  { description: '查看网关状态' },
  '/start':   { description: '显示欢迎信息' },
  '/clear':   { description: '清除会话说明' },
  '/shutdown': { description: '关闭守护进程' },
  '/quit':     { description: '断开 Telegram 网关（Daemon 保持运行）' },
  '/memory':   { description: '管理长期记忆' },
  '/cron':     { description: '定时任务: /cron add|list|remove|toggle' },
  '/cd':       { description: '切换工作目录: /cd <path>' },
  '/task':     { description: '后台任务: /task run|list|status|stop' },
};

// ═══════════════════════════════════════════════════════════════
// Command Parsing
// ═══════════════════════════════════════════════════════════════

/**
 * Parse a message text into a command name and arguments.
 * Returns null if the text is not a slash command.
 */
export function parseCommand(text: string): { command: string; args: string } | null {
  if (!text.startsWith('/')) return null;
  const trimmed = text.trim();
  const spaceIdx = trimmed.indexOf(' ');
  if (spaceIdx === -1) {
    return { command: trimmed.toLowerCase(), args: '' };
  }
  return {
    command: trimmed.slice(0, spaceIdx).toLowerCase(),
    args: trimmed.slice(spaceIdx + 1).trim(),
  };
}

/**
 * Check whether a message text starts with a registered command.
 */
export function isRegisteredCommand(text: string): boolean {
  const parsed = parseCommand(text);
  if (!parsed) return false;
  return parsed.command in COMMAND_REGISTRY;
}

// ═══════════════════════════════════════════════════════════════
// List Format Functions (Task 2)
// ═══════════════════════════════════════════════════════════════

/**
 * Format a list of registered tools as plain text.
 */
export function formatTools(tools: ToolInfo[], count: number): string {
  if (count === 0) return '暂无已注册的工具。';
  let out = `🔧 已注册工具 (${count})\n\n`;
  for (const t of tools) {
    out += `• ${t.name} [${t.type}]\n`;
    if (t.description) {
      const desc = t.description.length > 80 ? t.description.slice(0, 80) + '...' : t.description;
      out += `  ${desc}\n`;
    }
  }
  return out;
}

/**
 * Format a list of loaded skills as plain text.
 */
export function formatSkills(skills: SkillInfo[], count: number): string {
  if (count === 0) return '暂无已加载的技能。';
  let out = `📚 已加载技能 (${count})\n\n`;
  for (const s of skills) {
    out += `• ${s.name} [${s.source}]\n`;
    if (s.description) {
      out += `  ${s.description}\n`;
    }
  }
  return out;
}

/**
 * Format a list of MCP servers as plain text.
 */
export function formatMcpServers(servers: McpServerInfo[], count: number): string {
  if (count === 0) return '暂无已配置的 MCP 服务器。';
  let out = `🌐 MCP 服务器 (${count})\n\n`;
  for (const srv of servers) {
    const status = srv.disabled ? '🔴 已禁用' : '🟢 已启用';
    out += `• ${srv.name} [${srv.server_type}] ${status} [${srv.source}]\n`;
  }
  return out;
}

/**
 * Format a list of installed plugins as plain text.
 */
export function formatPlugins(plugins: PluginInfo[], count: number): string {
  if (count === 0) return '暂无已安装的插件。';
  let out = `🧩 已安装插件 (${count})\n\n`;
  for (const p of plugins) {
    const ver = p.version ? ` v${p.version}` : '';
    const features: string[] = [];
    if (p.has_tools) features.push('tools');
    if (p.has_skills) features.push('skills');
    if (p.has_mcp) features.push('mcp');
    const featureStr = features.length > 0 ? ` (${features.join(', ')})` : '';
    out += `• ${p.name}${ver} [${p.source}]${featureStr}\n`;
    if (p.description) {
      out += `  ${p.description}\n`;
    }
  }
  return out;
}

// ═══════════════════════════════════════════════════════════════
// Scalar Format Functions (Task 3)
// ═══════════════════════════════════════════════════════════════

/**
 * Format compact result showing tokens saved and summary tokens.
 */
export function formatCompact(result: CompactResult): string {
  return `🗜️ 上下文已压缩\n\n压缩前: ${result.tokens_before} tokens\n压缩后: ${result.tokens_after} tokens\n节省: ${result.tokens_saved} tokens\n摘要: ${result.summary_tokens} tokens`;
}

/**
 * Format git status showing branch, staged, modified, and untracked files.
 */
export function formatGitStatus(result: GitStatusResult): string {
  const branch = result.branch ?? '(detached)';
  let out = `📂 Git 状态\n\n分支: ${branch}\n`;
  if (result.staged_files.length > 0) {
    out += `\n暂存文件 (${result.staged_files.length}):\n`;
    for (const f of result.staged_files) out += `  ✅ ${f}\n`;
  }
  if (result.modified_files.length > 0) {
    out += `\n已修改文件 (${result.modified_files.length}):\n`;
    for (const f of result.modified_files) out += `  ✏️ ${f}\n`;
  }
  if (result.untracked_files.length > 0) {
    out += `\n未跟踪文件 (${result.untracked_files.length}):\n`;
    for (const f of result.untracked_files) out += `  ❓ ${f}\n`;
  }
  if (result.staged_files.length === 0 && result.modified_files.length === 0 && result.untracked_files.length === 0) {
    out += '\n工作区干净，无变更。';
  }
  return out;
}

/**
 * Format git diff output. Returns friendly message when empty.
 */
export function formatGitDiff(result: GitDiffResult): string {
  if (!result.diff || result.diff.trim() === '') return '无变更。';
  return `📝 Git Diff\n\n${result.diff}`;
}

/**
 * Format git commit result showing hash and message.
 */
export function formatGitCommit(result: GitCommitResult): string {
  return `✅ 提交成功\n\nHash: ${result.hash}\n消息: ${result.message}`;
}

/**
 * Format think toggle status with optional budget.
 */
export function formatThinkToggle(enabled: boolean, budget?: number): string {
  if (enabled) {
    const budgetStr = budget != null ? ` (预算: ${budget} tokens)` : '';
    return `🧠 扩展思考已开启${budgetStr}`;
  }
  return '🧠 扩展思考已关闭';
}

/**
 * Format model info showing active model and fallback chain.
 */
export function formatModelInfo(activeModel: string, fallbackModels: string[]): string {
  let out = `🤖 模型配置\n\n当前模型: ${activeModel}\n`;
  if (fallbackModels.length > 0) {
    out += `\n回退链:\n`;
    out += `  0. ${activeModel} (主模型)\n`;
    for (let i = 0; i < fallbackModels.length; i++) {
      out += `  ${i + 1}. ${fallbackModels[i]}\n`;
    }
  } else {
    out += '\n未配置回退模型。';
  }
  return out;
}

/**
 * Format model switch confirmation.
 */
export function formatModelSwitch(model: string): string {
  return `✅ 已切换到模型: ${model}`;
}

/**
 * Format commit usage hint when no message is provided.
 */
export function formatCommitUsage(): string {
  return '用法: /commit <message>';
}

/**
 * Format abort confirmation message.
 */
export function formatAbortConfirm(): string {
  return '⛔ 当前任务已中止。';
}

// ═══════════════════════════════════════════════════════════════
// Error and Help Functions (Task 4)
// ═══════════════════════════════════════════════════════════════

/**
 * Format an error message. Always starts with ❌ and includes error details.
 */
export function formatError(err: unknown): string {
  if (err instanceof Error) {
    return `❌ 命令失败: ${err.message}`;
  }
  return `❌ 命令失败: ${String(err)}`;
}

/**
 * Format a daemon disconnected warning.
 */
export function formatDisconnected(): string {
  return '⚠️ Daemon 连接已断开，请重启网关。';
}

/**
 * Format help output listing all commands with descriptions.
 */
export function formatHelp(registry: Record<string, { description: string }>): string {
  let out = '📖 可用命令\n\n';
  for (const [cmd, def] of Object.entries(registry)) {
    out += `${cmd} — ${def.description}\n`;
  }
  return out;
}

// ═══════════════════════════════════════════════════════════════
// Session Status & Start Functions (Task 6)
// ═══════════════════════════════════════════════════════════════

/**
 * Format the /status command output including session resume info.
 */
export function formatStatus(
  daemonInfo: { pid: number; session_id: string; cwd: string },
  botUsername: string,
  sessionState: SessionState,
): string {
  const sessionLine = sessionState.resumed
    ? `🔄 已恢复会话 (${sessionState.messageCount} 条消息)`
    : '🆕 新会话';
  return (
    `🐾 Status\n` +
    `Daemon: pid=${daemonInfo.pid}\n` +
    `Session: ${daemonInfo.session_id}\n` +
    `CWD: ${daemonInfo.cwd}\n` +
    `Bot: @${botUsername}\n` +
    sessionLine
  );
}

/**
 * Format the /start welcome message including session resume info.
 */
export function formatStart(
  daemonInfo: { pid: number; session_id: string },
  chatId: number,
  sessionState: SessionState,
): string {
  let msg =
    `🐾 BaoClaw Telegram Gateway\n\n` +
    `Connected to daemon pid=${daemonInfo.pid}\n` +
    `Session: ${daemonInfo.session_id}\n` +
    `Your chat ID: ${chatId}\n\n` +
    `Send me any message to chat with BaoClaw.`;
  if (sessionState.resumed) {
    msg += `\n\n🔄 已恢复之前的对话 (${sessionState.messageCount} 条消息)`;
  }
  return msg;
}
