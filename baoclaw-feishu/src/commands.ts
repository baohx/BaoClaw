/**
 * Command system for BaoClaw Feishu Gateway.
 * Adapts WhatsApp's commands.ts — same registry, same handlers,
 * but uses chatId/sendReply instead of jid/sock.
 */
import { IpcClient } from './ipcClient.js';
import { logger } from './log.js';
import * as fs from 'fs';
import * as os from 'os';

const MAX_OUTPUT = 4000;

// ── Adapted CommandContext for Feishu ──

export interface CommandContext {
  ipcClient: IpcClient;
  args: string;
  sender: string;
  chatId: string;
  sendReply: (text: string) => Promise<void>;
}

interface ParsedCommand {
  name: string;
  args: string;
}

// ── RPC Response Types ──

interface ToolInfo { name: string; description: string; type: string; }
interface SkillInfo { name: string; path: string; source: string; description?: string; }
interface McpServerInfo { name: string; server_type: string; disabled: boolean; source: string; command?: string; url?: string; config_path: string; }
interface PluginInfo { name: string; version?: string; description?: string; path: string; source: string; has_tools: boolean; has_skills: boolean; has_mcp: boolean; }
interface CompactResult { tokens_saved: number; summary_tokens: number; tokens_before: number; tokens_after: number; }
interface GitStatusResult { branch: string | null; has_changes: boolean; staged_files: string[]; modified_files: string[]; untracked_files: string[]; }
interface GitCommitResult { hash: string; message: string; }
interface GitDiffResult { diff: string; }
interface SearchResult { timestamp?: string; entry_type: string; snippet?: string; context?: string; }
interface HistoryEntry { role: string; content: string; timestamp?: string; }
interface ExportResult { path: string; size?: number; }
interface TaskInfo { id: string; description: string; status: string; created_at?: string; }
interface CronEntry { id: string; schedule: string; command: string; enabled: boolean; }
interface ProjectInfo { id: string; name: string; path: string; description?: string; }
interface SpecInfo { name: string; phase: string; total_tasks: number; completed_tasks: number; }

// ── Formatting Helpers ──

function truncate(text: string, limit: number = MAX_OUTPUT): string {
  if (text.length <= limit) return text;
  return text.slice(0, limit) + '\n…(输出已截断)';
}

function formatTools(tools: ToolInfo[]): string {
  const count = tools.length;
  if (count === 0) return '📋 已注册工具 (0)\n暂无已注册的工具。';
  const groups: Record<string, ToolInfo[]> = {};
  for (const t of tools) { const type = t.type || 'other'; if (!groups[type]) groups[type] = []; groups[type].push(t); }
  let out = `📋 已注册工具 (${count})\n`;
  for (const [type, items] of Object.entries(groups)) {
    out += `\n── ${type} (${items.length}) ──\n`;
    for (const t of items) {
      const desc = t.description ? (t.description.length > 60 ? t.description.slice(0, 60) + '…' : t.description) : '';
      out += `• ${t.name}  ${desc}\n`;
    }
  }
  return truncate(out);
}

function formatSkills(skills: SkillInfo[]): string {
  if (skills.length === 0) return '📋 已加载技能 (0)';
  let out = `📋 已加载技能 (${skills.length})\n`;
  for (const s of skills) { out += `• ${s.name} [${s.source}]\n`; if (s.description) out += `  ${s.description}\n`; }
  return truncate(out);
}

function formatMcpServers(servers: McpServerInfo[]): string {
  if (servers.length === 0) return '📋 MCP 服务器 (0)';
  let out = `📋 MCP 服务器 (${servers.length})\n`;
  for (const srv of servers) {
    const status = srv.disabled ? '🔴' : '🟢';
    out += `${status} ${srv.name} [${srv.server_type}] [${srv.source}]\n`;
  }
  return truncate(out);
}

function formatPlugins(plugins: PluginInfo[]): string {
  if (plugins.length === 0) return '📋 已安装插件 (0)';
  let out = `📋 已安装插件 (${plugins.length})\n`;
  for (const p of plugins) {
    const ver = p.version ? ` v${p.version}` : '';
    out += `• ${p.name}${ver} [${p.source}]\n`;
  }
  return truncate(out);
}

function formatCompact(result: CompactResult): string {
  const pct = result.tokens_before > 0 ? ((result.tokens_saved / result.tokens_before) * 100).toFixed(0) : '0';
  return `✅ 上下文已压缩\n\n压缩前 ${result.tokens_before.toLocaleString()} tokens\n压缩后 ${result.tokens_after.toLocaleString()} tokens\n节省 ${result.tokens_saved.toLocaleString()} tokens (${pct}%)\n摘要 ${result.summary_tokens.toLocaleString()} tokens`;
}

function formatGitStatus(result: GitStatusResult): string {
  let out = `📂 Git 状态\n\n分支: ${result.branch ?? '(detached)'}\n`;
  if (result.staged_files.length) { out += `\n暂存 (${result.staged_files.length}):\n`; for (const f of result.staged_files) out += `  ✅ ${f}\n`; }
  if (result.modified_files.length) { out += `\n已修改 (${result.modified_files.length}):\n`; for (const f of result.modified_files) out += `  ✏️ ${f}\n`; }
  if (result.untracked_files.length) { out += `\n未跟踪 (${result.untracked_files.length}):\n`; for (const f of result.untracked_files) out += `  ❓ ${f}\n`; }
  if (!result.has_changes) out += '\n工作区干净，无变更。';
  return out;
}

function formatGitDiff(result: GitDiffResult): string {
  return result.diff ? truncate(`📝 Git Diff\n\n${result.diff}`) : '无变更。';
}

function formatGitCommit(result: GitCommitResult): string {
  return `✅ 提交成功\n\nHash: ${result.hash}\n消息: ${result.message}`;
}

function formatHistory(entries: HistoryEntry[]): string {
  if (!entries?.length) return '暂无对话历史。';
  let out = `📜 最近对话 (${entries.length})\n\n`;
  for (const e of entries) {
    const role = e.role === 'user' ? '👤' : '🤖';
    const content = e.content.length > 100 ? e.content.slice(0, 100) + '…' : e.content;
    out += `${role} ${content}\n\n`;
    if (out.length > MAX_OUTPUT) { out += '…'; break; }
  }
  return out;
}

function formatSearchResults(results: SearchResult[], query: string): string {
  if (!results?.length) return `未找到匹配 "${query}" 的内容`;
  let out = `🔍 搜索结果: "${query}" (${results.length})\n\n`;
  for (const r of results) { out += `${r.snippet || r.context || ''}\n\n`; if (out.length > MAX_OUTPUT) { out += '…'; break; } }
  return out;
}

function formatExport(result: ExportResult): string {
  return `📤 导出成功\n\n路径: ${result.path}${result.size ? `\n大小: ${(result.size / 1024).toFixed(1)} KB` : ''}`;
}

function formatProjects(projects: ProjectInfo[]): string {
  if (!projects?.length) return '📋 项目列表 (0)';
  let out = `📋 项目列表 (${projects.length})\n\n`;
  for (const p of projects) { out += `• ${p.name} [${p.id}]\n  ${p.path}\n\n`; }
  return truncate(out);
}

function formatTasks(tasks: TaskInfo[]): string {
  if (!tasks?.length) return '📋 后台任务 (0)';
  let out = `📋 后台任务 (${tasks.length})\n\n`;
  for (const t of tasks) {
    const emoji = t.status === 'running' ? '🟢' : t.status === 'completed' ? '✅' : t.status === 'failed' ? '🔴' : '⚪';
    out += `${emoji} [${t.id}] ${t.description}\n  状态: ${t.status}\n\n`;
  }
  return truncate(out);
}

function formatCronList(crons: CronEntry[]): string {
  if (!crons?.length) return '📋 定时任务 (0)';
  let out = `📋 定时任务 (${crons.length})\n\n`;
  for (const c of crons) { const s = c.enabled ? '🟢' : '🔴'; out += `${s} [${c.id}] ${c.schedule} ${c.command}\n`; }
  return truncate(out);
}

function formatSpecList(specs: SpecInfo[]): string {
  if (!specs?.length) return '📋 Specs (0)';
  let out = `📋 Specs (${specs.length})\n\n`;
  for (const s of specs) { out += `• ${s.name} [${s.phase}] (${s.completed_tasks}/${s.total_tasks})\n`; }
  return out;
}

function formatSpecShow(spec: { name: string; content: string; phase: string }): string {
  return truncate(`📋 Spec: ${spec.name}\n阶段: ${spec.phase}\n\n${spec.content}`);
}

function formatSpecStatus(spec: { name: string; phase: string; tasks: { name: string; status: string }[] }): string {
  let out = `📊 Spec 状态: ${spec.name}\n阶段: ${spec.phase}\n\n`;
  for (const t of spec.tasks) { const e = t.status === 'completed' ? '✅' : t.status === 'in_progress' ? '🔄' : '⬜'; out += `${e} ${t.name}\n`; }
  return truncate(out);
}

function formatError(title: string, detail: string): string {
  return `❌ ${title}\n${detail}`;
}

// ── Daemon info (set by gateway) ──

let _daemonInfo: { pid: number; session_id: string; cwd: string } | null = null;

export function setDaemonInfo(info: typeof _daemonInfo): void { _daemonInfo = info; }

// ── Gateway Info Store ──

export interface GatewayInfo {
  pid: number;
  startTime: number;
  logFile: string;
  name: string;
}

let _gatewayInfo: GatewayInfo | null = null;

export function setGatewayInfo(info: GatewayInfo): void { _gatewayInfo = info; }

// ── Command Handlers ──

async function handleCompact(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<CompactResult>('compact');
  return formatCompact(result);
}

async function handleModel(ctx: CommandContext): Promise<string> {
  if (!ctx.args.trim()) return '当前模型信息请直接问 AI。\n用法: /model <模型名称>';
  const result = await ctx.ipcClient.request<{ model: string }>('switchModel', { model: ctx.args.trim() });
  return `✅ 已切换到模型: ${result.model ?? ctx.args.trim()}`;
}

async function handleHistory(ctx: CommandContext): Promise<string> {
  const n = parseInt(ctx.args.trim(), 10) || 10;
  const result = await ctx.ipcClient.request<{ entries: HistoryEntry[] }>('talkTail', { n });
  return formatHistory(result.entries ?? (result as unknown as HistoryEntry[]));
}

async function handleSearch(ctx: CommandContext): Promise<string> {
  if (!ctx.args.trim()) return formatError('参数缺失', '用法: /search <关键词>');
  const result = await ctx.ipcClient.request<SearchResult[]>('searchHistory', { query: ctx.args.trim() });
  return formatSearchResults(result, ctx.args.trim());
}

async function handleExport(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<ExportResult>('export');
  return formatExport(result);
}

async function handleAbort(ctx: CommandContext): Promise<string> {
  await ctx.ipcClient.request('abort');
  return '⛔ 当前任务已中止。';
}

async function handleGit(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<GitStatusResult>('gitStatus');
  return formatGitStatus(result);
}

async function handleDiff(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<GitDiffResult>('gitDiff');
  return formatGitDiff(result);
}

async function handleCommit(ctx: CommandContext): Promise<string> {
  if (!ctx.args.trim()) return formatError('参数缺失', '用法: /commit <提交消息>');
  const result = await ctx.ipcClient.request<GitCommitResult>('gitCommit', { message: ctx.args.trim() });
  return formatGitCommit(result);
}

async function handleTools(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<{ tools: ToolInfo[] } | ToolInfo[]>('listTools');
  const tools = Array.isArray(result) ? result : (result as any).tools ?? [];
  return formatTools(tools);
}

async function handleMcp(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<{ servers: McpServerInfo[] } | McpServerInfo[]>('listMcpServers');
  const servers = Array.isArray(result) ? result : (result as any).servers ?? [];
  return formatMcpServers(servers);
}

async function handleSkills(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<{ skills: SkillInfo[] } | SkillInfo[]>('listSkills');
  const skills = Array.isArray(result) ? result : (result as any).skills ?? [];
  return formatSkills(skills);
}

async function handlePlugins(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<{ plugins: PluginInfo[] } | PluginInfo[]>('listPlugins');
  const plugins = Array.isArray(result) ? result : (result as any).plugins ?? [];
  return formatPlugins(plugins);
}

async function handleProjects(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<{ projects: ProjectInfo[] } | ProjectInfo[]>('projectsList');
  const projects = Array.isArray(result) ? result : (result as any).projects ?? [];
  return formatProjects(projects);
}

async function handleTask(ctx: CommandContext): Promise<string> {
  if (!ctx.args.trim()) return formatError('参数缺失', '用法: /task <任务描述>');
  const result = await ctx.ipcClient.request<{ id: string; status: string }>('taskCreate', { description: ctx.args.trim() });
  return `🚀 任务已创建\n\nID: ${result.id}\n状态: ${result.status}`;
}

async function handleTasks(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<{ tasks: TaskInfo[] } | TaskInfo[]>('taskList');
  const tasks = Array.isArray(result) ? result : (result as any).tasks ?? [];
  return formatTasks(tasks);
}

async function handleTaskStop(ctx: CommandContext): Promise<string> {
  if (!ctx.args.trim()) return formatError('参数缺失', '用法: /task_stop <任务ID>');
  await ctx.ipcClient.request('taskStop', { id: ctx.args.trim() });
  return `⏹️ 任务已停止\n\nID: ${ctx.args.trim()}`;
}

async function handleCron(ctx: CommandContext): Promise<string> {
  const result = await ctx.ipcClient.request<{ crons: CronEntry[] } | CronEntry[]>('cronList');
  const crons = Array.isArray(result) ? result : (result as any).crons ?? [];
  return formatCronList(crons);
}

async function handleHelp(_ctx: CommandContext): Promise<string> {
  return formatHelp();
}

async function handleStatus(ctx: CommandContext): Promise<string> {
  const connected = ctx.ipcClient.connected ? '🟢 已连接' : '🔴 已断开';
  let out = `🐾 BaoClaw Feishu Gateway\n\nDaemon 连接: ${connected}\n`;
  if (_daemonInfo) { out += `Daemon PID: ${_daemonInfo.pid}\nSession: ${_daemonInfo.session_id}\nCWD: ${_daemonInfo.cwd}\n`; }
  return out;
}

async function handleStart(_ctx: CommandContext): Promise<string> {
  return '🐾 BaoClaw Feishu Gateway\n\n欢迎使用 BaoClaw！\n\n直接发送消息与 AI 对话，或使用 / 命令操作。\n输入 /help 查看所有可用命令。';
}

async function handleGateway(_ctx: CommandContext): Promise<string> {
  const args = _ctx.args.trim();
  const parts = args.split(/\s+/);
  const sub = parts[0] || 'status';

  switch (sub) {
    case 'status': {
      if (!_gatewayInfo) return '⚠️ 网关信息未初始化';
      const uptime = Math.floor((Date.now() - _gatewayInfo.startTime) / 1000);
      const mem = process.memoryUsage();
      let out = `🐾 ${_gatewayInfo.name} Gateway\n\n`;
      out += `PID: ${_gatewayInfo.pid}\n`;
      out += `运行时间: ${Math.floor(uptime / 3600)}h ${Math.floor((uptime % 3600) / 60)}m ${uptime % 60}s\n`;
      out += `内存 RSS: ${(mem.rss / 1024 / 1024).toFixed(1)} MB\n`;
      out += `内存 Heap: ${(mem.heapUsed / 1024 / 1024).toFixed(1)} / ${(mem.heapTotal / 1024 / 1024).toFixed(1)} MB\n`;
      out += `Node.js: ${process.version}\n`;
      out += `Platform: ${os.platform()} ${os.arch()}\n`;
      out += `系统运行: ${Math.floor(os.uptime() / 3600)}h\n`;
      out += `日志: ${_gatewayInfo.logFile}\n`;
      out += `Daemon: ${_daemonInfo ? `🟢 pid=${_daemonInfo.pid}` : '🔴 未连接'}\n`;
      return out;
    }
    case 'ping':
      return '🏓 pong! Gateway is alive.';
    case 'logs': {
      if (!_gatewayInfo) return '⚠️ 网关信息未初始化';
      const n = parseInt(parts[1], 10) || 10;
      try {
        if (!fs.existsSync(_gatewayInfo.logFile)) return '⚠️ 日志文件不存在';
        const content = fs.readFileSync(_gatewayInfo.logFile, 'utf-8');
        const lines = content.trim().split('\n');
        const recent = lines.slice(-Math.min(n, 50));
        if (recent.length === 0) return '📄 日志为空';
        return `📄 最近 ${recent.length} 条日志\n\n${recent.join('\n').slice(0, 3000)}`;
      } catch (e: any) {
        return `⚠️ 无法读取日志: ${e.message}`;
      }
    }
    default:
      return '📋 Gateway 命令\n\n• /gateway status — 运行状态\n• /gateway ping — 连通测试\n• /gateway logs [n] — 最近 n 条日志';
  }
}

async function handleThink(_ctx: CommandContext): Promise<string> {
  return '🧠 扩展思考\n\n直接发送消息描述需要深入思考的内容即可。';
}

async function handleSpec(ctx: CommandContext): Promise<string> {
  const parts = ctx.args.trim().split(/\s+/);
  const sub = parts[0] || '';
  const rest = parts.slice(1).join(' ');

  switch (sub) {
    case 'list': {
      const result = await ctx.ipcClient.request<{ specs: SpecInfo[] } | SpecInfo[]>('specList');
      const specs = Array.isArray(result) ? result : (result as any).specs ?? [];
      return formatSpecList(specs);
    }
    case 'new': {
      if (!rest) return formatError('参数缺失', '用法: /spec new <name>');
      const result = await ctx.ipcClient.request<{ name: string; phase: string }>('specNew', { name: rest });
      return `✅ Spec 已创建\n\n名称: ${result.name}\n阶段: ${result.phase}`;
    }
    case 'show': {
      if (!rest) return formatError('参数缺失', '用法: /spec show <name>');
      const result = await ctx.ipcClient.request<{ name: string; content: string; phase: string }>('specShow', { name: rest });
      return formatSpecShow(result);
    }
    case 'status': {
      if (!rest) return formatError('参数缺失', '用法: /spec status <name>');
      const result = await ctx.ipcClient.request<{ name: string; phase: string; tasks: { name: string; status: string }[] }>('specStatus', { name: rest });
      return formatSpecStatus(result);
    }
    case 'run': {
      const name = parts[1]; const taskId = parts[2];
      if (!name) return formatError('参数缺失', '用法: /spec run <name> [task_id]');
      const params: Record<string, string> = { name }; if (taskId) params.task_id = taskId;
      const result = await ctx.ipcClient.request<{ task_id?: string; status: string; message?: string }>('specRun', params);
      return result.message ? `🚀 Spec 执行\n\n${result.message}` : `🚀 Spec 已开始执行\n\n任务 ID: ${result.task_id || 'N/A'}\n状态: ${result.status}`;
    }
    default:
      return '📋 Spec 命令\n\n• /spec list — 列出所有\n• /spec new <name> — 创建\n• /spec show <name> — 详情\n• /spec status <name> — 状态\n• /spec run <name> — 执行';
  }
}

// ── Command Registry ──

interface CommandDef {
  name: string;
  description: string;
  usage?: string;
  handler: (ctx: CommandContext) => Promise<string>;
}

export const COMMAND_REGISTRY: Record<string, CommandDef> = {
  '/compact':   { name: '/compact',   description: '压缩对话上下文',                   handler: handleCompact },
  '/think':     { name: '/think',     description: '扩展思考模式提示',                  handler: handleThink },
  '/model':     { name: '/model',     description: '查看或切换模型',     usage: '/model [name]', handler: handleModel },
  '/history':   { name: '/history',   description: '查看最近对话',       usage: '/history [n]', handler: handleHistory },
  '/search':    { name: '/search',    description: '搜索对话历史',       usage: '/search <query>', handler: handleSearch },
  '/export':    { name: '/export',    description: '导出对话历史',                        handler: handleExport },
  '/abort':     { name: '/abort',     description: '中止当前任务',                        handler: handleAbort },
  '/git':       { name: '/git',       description: '查看 git 状态',                       handler: handleGit },
  '/diff':      { name: '/diff',      description: '查看 git diff',                       handler: handleDiff },
  '/commit':    { name: '/commit',    description: '提交 git 变更',     usage: '/commit <message>', handler: handleCommit },
  '/tools':     { name: '/tools',     description: '列出已注册的工具',                     handler: handleTools },
  '/mcp':       { name: '/mcp',       description: '列出 MCP 服务器',                      handler: handleMcp },
  '/skills':    { name: '/skills',    description: '列出已加载的技能',                     handler: handleSkills },
  '/plugins':   { name: '/plugins',   description: '列出已安装的插件',                     handler: handlePlugins },
  '/projects':  { name: '/projects',  description: '列出项目',                             handler: handleProjects },
  '/task':      { name: '/task',      description: '创建后台任务',       usage: '/task <description>', handler: handleTask },
  '/tasks':     { name: '/tasks',     description: '列出后台任务',                         handler: handleTasks },
  '/task_stop': { name: '/task_stop', description: '停止后台任务',       usage: '/task_stop <id>', handler: handleTaskStop },
  '/cron':      { name: '/cron',      description: '列出定时任务',                         handler: handleCron },
  '/help':      { name: '/help',      description: '显示帮助信息',                         handler: handleHelp },
  '/status':    { name: '/status',    description: '查看网关状态',                         handler: handleStatus },
  '/start':     { name: '/start',     description: '显示欢迎信息',                         handler: handleStart },
  '/gateway':   { name: '/gateway',   description: '网关管理（信息型）',  usage: '/gateway status|ping|logs', handler: handleGateway },
  '/spec':      { name: '/spec',      description: 'Spec 管理',          usage: '/spec list|new|show|status|run', handler: handleSpec },
};

// ── Command Parsing & Dispatch ──

export function parseCommand(text: string): ParsedCommand | null {
  if (!text.startsWith('/')) return null;
  const trimmed = text.trim();
  const spaceIdx = trimmed.indexOf(' ');
  if (spaceIdx === -1) return { name: trimmed.toLowerCase(), args: '' };
  return { name: trimmed.slice(0, spaceIdx).toLowerCase(), args: trimmed.slice(spaceIdx + 1).trim() };
}

export function isRegisteredCommand(name: string): boolean {
  return name in COMMAND_REGISTRY;
}

export async function dispatchCommand(cmd: ParsedCommand, ctx: CommandContext): Promise<string | null> {
  const command = COMMAND_REGISTRY[cmd.name];
  if (!command) return null;

  const fullCtx: CommandContext = { ...ctx, args: cmd.args };

  try {
    const result = await command.handler(fullCtx);
    return result;
  } catch (err: any) {
    const message = err instanceof Error ? err.message : String(err);
    logger.error(`Command ${cmd.name} failed: ${message}`);
    return formatError('命令失败', message);
  }
}

// ── Help Text ──

export function formatHelp(): string {
  const groups: [string, string[]][] = [
    ['💬 对话',        ['/compact', '/think', '/model', '/history', '/search', '/export', '/abort']],
    ['📂 项目 & Git',  ['/projects', '/git', '/diff', '/commit']],
    ['🔧 工具 & 扩展', ['/tools', '/mcp', '/skills', '/plugins']],
    ['⚙️ 自动化',      ['/task', '/tasks', '/task_stop', '/cron']],
    ['📋 Spec',        ['/spec']],
    ['🚪 网关',        ['/gateway']],
    ['🔌 会话',        ['/help', '/status', '/start']],
  ];

  let out = '📖 BaoClaw 命令列表\n\n';
  for (const [group, cmds] of groups) {
    out += `${group}\n`;
    for (const c of cmds) {
      const def = COMMAND_REGISTRY[c];
      if (def) out += `  ${c} — ${def.description}${def.usage ? ` ${def.usage}` : ''}\n`;
    }
    out += '\n';
  }
  out += '发送任意非命令消息即可与 AI 对话';
  return out;
}
