/**
 * Command system for BaoClaw WhatsApp Gateway.
 * Provides command definitions, parsing, dispatch, formatting, and help.
 * Commands are dispatched via IPC JSON-RPC to baoclaw-core daemon.
 */
import { IpcClient } from "../../ts-ipc/index.js";
import * as fs from "fs";
import * as os from "os";

// ═══════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════

const MAX_OUTPUT = 4000;

// ═══════════════════════════════════════════════════════════════
// Interface Definitions
// ═══════════════════════════════════════════════════════════════

export interface Command {
  name: string;
  description: string;
  usage?: string;
  handler: (ctx: CommandContext) => Promise<string | void>;
}

export interface CommandContext {
  ipcClient: IpcClient;
  args: string;
  sender: string;
  jid: string;
  sock: any;
}

// ═══════════════════════════════════════════════════════════════
// RPC Response Types
// ═══════════════════════════════════════════════════════════════

interface ToolInfo {
  name: string;
  description: string;
  type: string; // 'builtin' | 'mcp' | 'plugin'
}

interface SkillInfo {
  name: string;
  path: string;
  source: string; // 'project' | 'global'
  description?: string;
}

interface McpServerInfo {
  name: string;
  server_type: string; // 'stdio' | 'sse'
  disabled: boolean;
  source: string;
  command?: string;
  url?: string;
  config_path: string;
}

interface PluginInfo {
  name: string;
  version?: string;
  description?: string;
  path: string;
  source: string;
  has_tools: boolean;
  has_skills: boolean;
  has_mcp: boolean;
}

interface CompactResult {
  tokens_saved: number;
  summary_tokens: number;
  tokens_before: number;
  tokens_after: number;
}

interface GitStatusResult {
  branch: string | null;
  has_changes: boolean;
  staged_files: string[];
  modified_files: string[];
  untracked_files: string[];
}

interface GitCommitResult {
  hash: string;
  message: string;
}

interface GitDiffResult {
  diff: string;
}

interface SearchResult {
  timestamp?: string;
  entry_type: string;
  snippet?: string;
  context?: string;
}

interface HistoryEntry {
  role: string;
  content: string;
  timestamp?: string;
}

interface ExportResult {
  path: string;
  size?: number;
}

interface TaskInfo {
  id: string;
  description: string;
  status: string;
  created_at?: string;
}

interface CronEntry {
  id: string;
  schedule: string;
  command: string;
  enabled: boolean;
}

interface ProjectInfo {
  id: string;
  name: string;
  path: string;
  description?: string;
}

interface SpecInfo {
  name: string;
  phase: string;
  total_tasks: number;
  completed_tasks: number;
}

// ═══════════════════════════════════════════════════════════════
// Private Formatting Helpers
// ═══════════════════════════════════════════════════════════════

/** Truncate text to MAX_OUTPUT characters with ellipsis indicator. */
function truncate(text: string, limit: number = MAX_OUTPUT): string {
  if (text.length <= limit) return text;
  return text.slice(0, limit) + "\n…(输出已截断)";
}

/** Format a generic list (tools/skills/mcp/plugins). */
function formatItemList(
  emoji: string,
  title: string,
  items: string[],
  count: number,
): string {
  if (count === 0) return `${emoji} *${title}*\n暂无内容`;
  let out = `📋 *${title}* (${count})\n`;
  for (const item of items) {
    out += `• ${item}\n`;
  }
  return truncate(out);
}

function formatTools(tools: ToolInfo[]): string {
  const count = tools.length;
  if (count === 0) return "📋 *已注册工具* (0)\n暂无已注册的工具。";

  // Group by type
  const groups: Record<string, ToolInfo[]> = {};
  for (const t of tools) {
    const type = t.type || "other";
    if (!groups[type]) groups[type] = [];
    groups[type].push(t);
  }

  let out = `📋 *已注册工具* (${count})\n`;
  for (const [type, items] of Object.entries(groups)) {
    out += `\n── ${type} (${items.length}) ──\n`;
    for (const t of items) {
      const desc = t.description
        ? t.description.length > 60
          ? t.description.slice(0, 60) + "…"
          : t.description
        : "";
      out += `• ${t.name}  ${desc}\n`;
    }
  }
  return truncate(out);
}

function formatSkills(skills: SkillInfo[]): string {
  const count = skills.length;
  if (count === 0) return "📋 *已加载技能* (0)\n暂无已加载的技能。";
  let out = `📋 *已加载技能* (${count})\n`;
  for (const s of skills) {
    out += `• ${s.name} [${s.source}]\n`;
    if (s.description) {
      out += `  ${s.description}\n`;
    }
  }
  return truncate(out);
}

function formatMcpServers(servers: McpServerInfo[]): string {
  const count = servers.length;
  if (count === 0) return "📋 *MCP 服务器* (0)\n暂无已配置的 MCP 服务器。";
  let out = `📋 *MCP 服务器* (${count})\n`;
  for (const srv of servers) {
    const status = srv.disabled ? "🔴" : "🟢";
    out += `${status} ${srv.name}  [${srv.server_type}] [${srv.source}]\n`;
  }
  return truncate(out);
}

function formatPlugins(plugins: PluginInfo[]): string {
  const count = plugins.length;
  if (count === 0) return "📋 *已安装插件* (0)\n暂无已安装的插件。";
  let out = `📋 *已安装插件* (${count})\n`;
  for (const p of plugins) {
    const ver = p.version ? ` v${p.version}` : "";
    const features: string[] = [];
    if (p.has_tools) features.push("tools");
    if (p.has_skills) features.push("skills");
    if (p.has_mcp) features.push("mcp");
    const featureStr = features.length > 0 ? ` (${features.join(", ")})` : "";
    out += `• ${p.name}${ver} [${p.source}]${featureStr}\n`;
    if (p.description) {
      out += `  ${p.description}\n`;
    }
  }
  return truncate(out);
}

function formatCompact(result: CompactResult): string {
  const pct =
    result.tokens_before > 0
      ? ((result.tokens_saved / result.tokens_before) * 100).toFixed(0)
      : "0";
  return (
    `✅ *上下文已压缩*\n\n` +
    `压缩前  ${result.tokens_before.toLocaleString()} tokens\n` +
    `压缩后  ${result.tokens_after.toLocaleString()} tokens\n` +
    `节省    ${result.tokens_saved.toLocaleString()} tokens (${pct}%)\n` +
    `摘要    ${result.summary_tokens.toLocaleString()} tokens`
  );
}

function formatGitStatus(result: GitStatusResult): string {
  const branch = result.branch ?? "(detached)";
  let out = `📂 *Git 状态*\n\n分支: *${branch}*\n`;
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
  if (
    result.staged_files.length === 0 &&
    result.modified_files.length === 0 &&
    result.untracked_files.length === 0
  ) {
    out += "\n工作区干净，无变更。";
  }
  return out;
}

function formatGitDiff(result: GitDiffResult): string {
  if (!result.diff || result.diff.trim() === "") return "无变更。";
  return truncate(`📝 *Git Diff*\n\n\`\`\`\n${result.diff}\n\`\`\``);
}

function formatGitCommit(result: GitCommitResult): string {
  return `✅ *提交成功*\n\nHash: \`${result.hash}\`\n消息: ${result.message}`;
}

function formatHistory(entries: HistoryEntry[], count: number): string {
  if (!entries || entries.length === 0) return "暂无对话历史。";
  let out = `📜 *最近对话* (${entries.length})\n\n`;
  for (const e of entries) {
    const role = e.role === "user" ? "👤" : "🤖";
    const content =
      e.content.length > 100 ? e.content.slice(0, 100) + "…" : e.content;
    out += `${role} ${content}\n\n`;
    if (out.length > MAX_OUTPUT) {
      out += "…(更多已截断)";
      break;
    }
  }
  return out;
}

function formatSearchResults(results: SearchResult[], query: string): string {
  if (!results || results.length === 0) return `未找到匹配 "${query}" 的内容`;
  let out = `🔍 *搜索结果*: "${query}" (${results.length})\n\n`;
  for (const r of results) {
    const ts = r.timestamp?.slice(0, 19).replace("T", " ") || "";
    const role = r.entry_type === "UserMessage" ? "👤" : "🤖";
    out += `[${ts}] ${role}\n${r.snippet || r.context || ""}\n\n`;
    if (out.length > MAX_OUTPUT) {
      out += "…(更多结果已截断)";
      break;
    }
  }
  return out;
}

function formatExport(result: ExportResult): string {
  return `📤 *导出成功*\n\n路径: ${result.path}${result.size ? `\n大小: ${(result.size / 1024).toFixed(1)} KB` : ""}`;
}

function formatProjects(projects: ProjectInfo[]): string {
  const count = projects.length;
  if (count === 0) return "📋 *项目列表* (0)\n暂无项目。";
  let out = `📋 *项目列表* (${count})\n\n`;
  for (const p of projects) {
    out += `• *${p.name}* [${p.id}]\n  ${p.path}\n`;
    if (p.description) out += `  ${p.description}\n`;
    out += "\n";
  }
  return truncate(out);
}

function formatTasks(tasks: TaskInfo[]): string {
  const count = tasks.length;
  if (count === 0) return "📋 *任务列表* (0)\n暂无后台任务。";
  let out = `📋 *后台任务* (${count})\n\n`;
  for (const t of tasks) {
    const statusEmoji =
      t.status === "running"
        ? "🟢"
        : t.status === "completed"
          ? "✅"
          : t.status === "failed"
            ? "🔴"
            : "⚪";
    out += `${statusEmoji} [${t.id}] ${t.description}\n  状态: ${t.status}\n\n`;
  }
  return truncate(out);
}

function formatCronList(crons: CronEntry[]): string {
  const count = crons.length;
  if (count === 0) return "📋 *定时任务* (0)\n暂无定时任务。";
  let out = `📋 *定时任务* (${count})\n\n`;
  for (const c of crons) {
    const status = c.enabled ? "🟢" : "🔴";
    out += `${status} [${c.id}] \`${c.schedule}\` ${c.command}\n`;
  }
  return truncate(out);
}

function formatSpecList(specs: SpecInfo[]): string {
  const count = specs.length;
  if (count === 0) return "📋 *Specs* (0)\n暂无 Spec。";
  let out = `📋 *Specs* (${count})\n\n`;
  for (const s of specs) {
    out += `• ${s.name} [${s.phase}] (${s.completed_tasks}/${s.total_tasks} tasks)\n`;
  }
  return out;
}

function formatSpecShow(spec: {
  name: string;
  content: string;
  phase: string;
}): string {
  let out = `📋 *Spec: ${spec.name}*\n阶段: ${spec.phase}\n\n`;
  out += spec.content;
  return truncate(out);
}

function formatSpecStatus(spec: {
  name: string;
  phase: string;
  tasks: { name: string; status: string }[];
}): string {
  let out = `📊 *Spec 状态: ${spec.name}*\n阶段: ${spec.phase}\n\n`;
  for (const t of spec.tasks) {
    const emoji =
      t.status === "completed"
        ? "✅"
        : t.status === "in_progress"
          ? "🔄"
          : "⬜";
    out += `${emoji} ${t.name} [${t.status}]\n`;
  }
  return truncate(out);
}

function formatSpecRun(result: {
  task_id?: string;
  status: string;
  message?: string;
}): string {
  if (result.message) {
    return `🚀 *Spec 执行*\n\n${result.message}`;
  }
  return `🚀 *Spec 已开始执行*\n\n任务 ID: ${result.task_id || "N/A"}\n状态: ${result.status}`;
}

function formatStatus(
  daemonInfo: { pid: number; session_id: string; cwd: string } | null,
  ipcClient: IpcClient,
): string {
  const connected = ipcClient.connected ? "🟢 已连接" : "🔴 已断开";
  let out = `🐾 *BaoClaw WhatsApp Gateway*\n\n`;
  out += `Daemon 连接: ${connected}\n`;
  if (daemonInfo) {
    out += `Daemon PID: ${daemonInfo.pid}\n`;
    out += `Session: ${daemonInfo.session_id}\n`;
    out += `CWD: ${daemonInfo.cwd}\n`;
  }
  out += `Reconnects: ${_daemonMetrics.reconnectCount}\n`;
  out += `Last connect: ${_daemonMetrics.lastConnectAt?.toISOString() ?? "never"}\n`;
  return out;
}

function formatError(title: string, detail: string): string {
  return `❌ *${title}*\n${detail}`;
}

// ═══════════════════════════════════════════════════════════════
// Command Definitions
// ═══════════════════════════════════════════════════════════════

// We need a mutable reference to daemonInfo for /status.
// This will be set by the gateway when it creates the commands module.
let _daemonInfo: { pid: number; session_id: string; cwd: string } | null = null;
let _daemonMetrics = { reconnectCount: 0, lastConnectAt: null as Date | null };

/** Set the daemon info reference (called by gateway after connection). */
export function setDaemonInfo(
  info: { pid: number; session_id: string; cwd: string } | null,
): void {
  _daemonInfo = info;
}

export function setDaemonMetrics(metrics: typeof _daemonMetrics): void {
  _daemonMetrics = metrics;
}

// ── Conversation Commands ──

const compactCommand: Command = {
  name: "/compact",
  description: "压缩对话上下文",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<CompactResult>("compact");
    return formatCompact(result);
  },
};

const thinkCommand: Command = {
  name: "/think",
  description: "扩展思考模式提示",
  async handler(_ctx) {
    return (
      "🧠 *扩展思考*\n\n" +
      "你可以直接发送消息描述需要深入思考的内容。\n" +
      "AI 会进行更详细的分析和推理。\n\n" +
      "例如：\n" +
      "• 直接发消息问一个复杂问题\n" +
      "• 要求分析某段代码的逻辑\n" +
      "• 请求逐步推理一个数学问题"
    );
  },
};

const modelCommand: Command = {
  name: "/model",
  description: "查看或切换模型",
  usage: "/model [name]",
  async handler(ctx) {
    if (!ctx.args.trim()) {
      return "🤖 *模型信息*\n\n当前模型信息请直接问 AI。\n\n用法: `/model <模型名称>` 切换模型";
    }
    const result = await ctx.ipcClient.request<{ model: string }>(
      "switchModel",
      { model: ctx.args.trim() },
    );
    return `✅ *已切换到模型:* ${result.model ?? ctx.args.trim()}`;
  },
};

const historyCommand: Command = {
  name: "/history",
  description: "查看最近对话",
  usage: "/history [n]",
  async handler(ctx) {
    const n = parseInt(ctx.args.trim(), 10) || 10;
    const result = await ctx.ipcClient.request<{ entries: HistoryEntry[] }>(
      "talkTail",
      { n },
    );
    return formatHistory(
      result.entries ?? (result as unknown as HistoryEntry[]),
      n,
    );
  },
};

const searchCommand: Command = {
  name: "/search",
  description: "搜索对话历史",
  usage: "/search <query>",
  async handler(ctx) {
    if (!ctx.args.trim()) {
      return formatError("参数缺失", "用法: /search <关键词>");
    }
    const result = await ctx.ipcClient.request<SearchResult[]>(
      "searchHistory",
      { query: ctx.args.trim() },
    );
    return formatSearchResults(result, ctx.args.trim());
  },
};

const exportCommand: Command = {
  name: "/export",
  description: "导出对话历史为文件",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<ExportResult>("export");
    const text = formatExport(result);
    // Return text + export path for gateway to handle file sending
    return text;
  },
};

const abortCommand: Command = {
  name: "/abort",
  description: "中止当前任务",
  async handler(ctx) {
    await ctx.ipcClient.request("abort");
    return "⛔ 当前任务已中止。";
  },
};

// ── Project & Git Commands ──

const projectsCommand: Command = {
  name: "/projects",
  description: "列出项目",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<
      { projects: ProjectInfo[] } | ProjectInfo[]
    >("projectsList");
    const projects = Array.isArray(result)
      ? result
      : ((result as any).projects ?? []);
    return formatProjects(projects);
  },
};

const gitCommand: Command = {
  name: "/git",
  description: "查看 git 状态",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<GitStatusResult>("gitStatus");
    return formatGitStatus(result);
  },
};

const diffCommand: Command = {
  name: "/diff",
  description: "查看 git diff",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<GitDiffResult>("gitDiff");
    return formatGitDiff(result);
  },
};

const commitCommand: Command = {
  name: "/commit",
  description: "提交 git 变更",
  usage: "/commit <message>",
  async handler(ctx) {
    if (!ctx.args.trim()) {
      return formatError(
        "参数缺失",
        "用法: /commit <提交消息>\n\n请提供提交消息，例如:\n/commit 修复登录页面样式问题",
      );
    }
    const result = await ctx.ipcClient.request<GitCommitResult>("gitCommit", {
      message: ctx.args.trim(),
    });
    return formatGitCommit(result);
  },
};

// ── Tools & Extensions Commands ──

const toolsCommand: Command = {
  name: "/tools",
  description: "列出已注册的工具",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<
      { tools: ToolInfo[] } | ToolInfo[]
    >("listTools");
    const tools = Array.isArray(result)
      ? result
      : ((result as any).tools ?? []);
    return formatTools(tools);
  },
};

const mcpCommand: Command = {
  name: "/mcp",
  description: "列出 MCP 服务器",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<
      { servers: McpServerInfo[] } | McpServerInfo[]
    >("listMcpServers");
    const servers = Array.isArray(result)
      ? result
      : ((result as any).servers ?? []);
    return formatMcpServers(servers);
  },
};

const skillsCommand: Command = {
  name: "/skills",
  description: "列出已加载的技能",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<
      { skills: SkillInfo[] } | SkillInfo[]
    >("listSkills");
    const skills = Array.isArray(result)
      ? result
      : ((result as any).skills ?? []);
    return formatSkills(skills);
  },
};

const pluginsCommand: Command = {
  name: "/plugins",
  description: "列出已安装的插件",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<
      { plugins: PluginInfo[] } | PluginInfo[]
    >("listPlugins");
    const plugins = Array.isArray(result)
      ? result
      : ((result as any).plugins ?? []);
    return formatPlugins(plugins);
  },
};

// ── Automation Commands ──

const taskCommand: Command = {
  name: "/task",
  description: "创建后台任务",
  usage: "/task <description>",
  async handler(ctx) {
    if (!ctx.args.trim()) {
      return formatError(
        "参数缺失",
        "用法: /task <任务描述>\n\n例如:\n/task 分析 src 目录下的代码质量",
      );
    }
    const result = await ctx.ipcClient.request<{ id: string; status: string }>(
      "taskCreate",
      { description: ctx.args.trim() },
    );
    return `🚀 *任务已创建*\n\nID: ${result.id}\n状态: ${result.status}`;
  },
};

const tasksCommand: Command = {
  name: "/tasks",
  description: "列出后台任务",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<
      { tasks: TaskInfo[] } | TaskInfo[]
    >("taskList");
    const tasks = Array.isArray(result)
      ? result
      : ((result as any).tasks ?? []);
    return formatTasks(tasks);
  },
};

const taskStopCommand: Command = {
  name: "/task_stop",
  description: "停止后台任务",
  usage: "/task_stop <id>",
  async handler(ctx) {
    if (!ctx.args.trim()) {
      return formatError("参数缺失", "用法: /task_stop <任务ID>");
    }
    await ctx.ipcClient.request("taskStop", { id: ctx.args.trim() });
    return `⏹️ *任务已停止*\n\nID: ${ctx.args.trim()}`;
  },
};

const cronCommand: Command = {
  name: "/cron",
  description: "列出定时任务",
  async handler(ctx) {
    const result = await ctx.ipcClient.request<
      { crons: CronEntry[] } | CronEntry[]
    >("cronList");
    const crons = Array.isArray(result)
      ? result
      : ((result as any).crons ?? []);
    return formatCronList(crons);
  },
};

// ── Session Commands ──

const helpCommand: Command = {
  name: "/help",
  description: "显示帮助信息",
  async handler(_ctx) {
    return formatHelp();
  },
};

const statusCommand: Command = {
  name: "/status",
  description: "查看网关状态",
  async handler(ctx) {
    return formatStatus(_daemonInfo, ctx.ipcClient);
  },
};

const startCommand: Command = {
  name: "/start",
  description: "显示欢迎信息",
  async handler(_ctx) {
    return (
      "🐾 *BaoClaw WhatsApp Gateway*\n\n" +
      "欢迎使用 BaoClaw！\n\n" +
      "你可以直接发送消息与 AI 对话，或使用 / 命令操作。\n\n" +
      "输入 `/help` 查看所有可用命令。"
    );
  },
};

const clearCommand: Command = {
  name: "/clear",
  description: "清除本地缓存",
  async handler(_ctx) {
    return "🧹 本地缓存已清除";
  },
};

// ── Gateway Info Store ──

export interface GatewayInfo {
  pid: number;
  startTime: number;
  logFile: string;
  name: string;
}

let _gatewayInfo: GatewayInfo | null = null;

export function setGatewayInfo(info: GatewayInfo): void {
  _gatewayInfo = info;
}

// ── Gateway Management Command ──

const gatewayCommand: Command = {
  name: "/gateway",
  description: "网关管理（信息型，不杀进程）",
  usage: "/gateway status|ping|logs [n]",
  async handler(_ctx) {
    const args = _ctx.args.trim();
    const parts = args.split(/\s+/);
    const sub = parts[0] || "status";

    switch (sub) {
      case "status": {
        if (!_gatewayInfo) return "⚠️ 网关信息未初始化";
        const uptime = Math.floor((Date.now() - _gatewayInfo.startTime) / 1000);
        const mem = process.memoryUsage();
        let out = `🐾 *${_gatewayInfo.name} Gateway*\n\n`;
        out += `PID: ${_gatewayInfo.pid}\n`;
        out += `运行时间: ${Math.floor(uptime / 3600)}h ${Math.floor((uptime % 3600) / 60)}m ${uptime % 60}s\n`;
        out += `内存 RSS: ${(mem.rss / 1024 / 1024).toFixed(1)} MB\n`;
        out += `内存 Heap: ${(mem.heapUsed / 1024 / 1024).toFixed(1)} / ${(mem.heapTotal / 1024 / 1024).toFixed(1)} MB\n`;
        out += `Node.js: ${process.version}\n`;
        out += `Platform: ${os.platform()} ${os.arch()}\n`;
        out += `系统运行: ${Math.floor(os.uptime() / 3600)}h\n`;
        out += `日志: ${_gatewayInfo.logFile}\n`;
        out += `Daemon: ${_daemonInfo ? `🟢 pid=${_daemonInfo.pid}` : "🔴 未连接"}\n`;
        return out;
      }
      case "ping":
        return "🏓 pong! Gateway is alive.";
      case "logs": {
        if (!_gatewayInfo) return "⚠️ 网关信息未初始化";
        const n = parseInt(parts[1], 10) || 10;
        try {
          if (!fs.existsSync(_gatewayInfo.logFile)) return "⚠️ 日志文件不存在";
          const content = fs.readFileSync(_gatewayInfo.logFile, "utf-8");
          const lines = content.trim().split("\n");
          const recent = lines.slice(-Math.min(n, 50));
          if (recent.length === 0) return "📄 日志为空";
          return `📄 *最近 ${recent.length} 条日志*\n\n\`\`\`\n${recent.join("\n").slice(0, 3000)}\n\`\`\``;
        } catch (e: any) {
          return `⚠️ 无法读取日志: ${e.message}`;
        }
      }
      default:
        return `📋 *Gateway 命令*\n\n• /gateway status — 网关运行状态\n• /gateway ping — 连通测试\n• /gateway logs [n] — 最近 n 条日志`;
    }
  },
};

// ── Spec Commands (subcommand system) ──

const specCommand: Command = {
  name: "/spec",
  description: "Spec 管理",
  usage: "/spec list|new|show|status|run",
  async handler(ctx) {
    const parts = ctx.args.trim().split(/\s+/);
    const sub = parts[0] || "";
    const rest = parts.slice(1).join(" ");

    switch (sub) {
      case "list": {
        const result = await ctx.ipcClient.request<
          { specs: SpecInfo[] } | SpecInfo[]
        >("specList");
        const specs = Array.isArray(result)
          ? result
          : ((result as any).specs ?? []);
        return formatSpecList(specs);
      }

      case "new": {
        if (!rest) {
          return formatError("参数缺失", "用法: /spec new <name>");
        }
        const result = await ctx.ipcClient.request<{
          name: string;
          phase: string;
        }>("specNew", { name: rest });
        return `✅ *Spec 已创建*\n\n名称: ${result.name}\n阶段: ${result.phase}`;
      }

      case "show": {
        if (!rest) {
          return formatError("参数缺失", "用法: /spec show <name>");
        }
        const result = await ctx.ipcClient.request<{
          name: string;
          content: string;
          phase: string;
        }>("specShow", { name: rest });
        return formatSpecShow(result);
      }

      case "status": {
        if (!rest) {
          return formatError("参数缺失", "用法: /spec status <name>");
        }
        const result = await ctx.ipcClient.request<{
          name: string;
          phase: string;
          tasks: { name: string; status: string }[];
        }>("specStatus", { name: rest });
        return formatSpecStatus(result);
      }

      case "run": {
        const name = parts[1];
        if (!name) {
          return formatError("参数缺失", "用法: /spec run <name> [task_id]");
        }
        const taskId = parts[2];
        const params: Record<string, string> = { name };
        if (taskId) params.task_id = taskId;
        const result = await ctx.ipcClient.request<{
          task_id?: string;
          status: string;
          message?: string;
        }>("specRun", params);
        return formatSpecRun(result);
      }

      default:
        return (
          "📋 *Spec 命令*\n\n" +
          "• `/spec list` — 列出所有 Specs\n" +
          "• `/spec new <name>` — 创建新 Spec\n" +
          "• `/spec show <name>` — 查看 Spec 详情\n" +
          "• `/spec status <name>` — 查看 Spec 状态\n" +
          "• `/spec run <name> [task_id]` — 执行 Spec"
        );
    }
  },
};

// ═══════════════════════════════════════════════════════════════
// Command Registry
// ═══════════════════════════════════════════════════════════════

export const COMMAND_REGISTRY: Record<string, Command> = {
  // Conversation
  "/compact": compactCommand,
  "/think": thinkCommand,
  "/model": modelCommand,
  "/history": historyCommand,
  "/search": searchCommand,
  "/export": exportCommand,
  "/abort": abortCommand,

  // Project & Git
  "/projects": projectsCommand,
  "/git": gitCommand,
  "/diff": diffCommand,
  "/commit": commitCommand,

  // Tools & Extensions
  "/tools": toolsCommand,
  "/mcp": mcpCommand,
  "/skills": skillsCommand,
  "/plugins": pluginsCommand,

  // Automation
  "/task": taskCommand,
  "/tasks": tasksCommand,
  "/task_stop": taskStopCommand,
  "/cron": cronCommand,

  // Session
  "/help": helpCommand,
  "/status": statusCommand,
  "/start": startCommand,
  "/clear": clearCommand,

  // Gateway
  "/gateway": gatewayCommand,

  // Spec
  "/spec": specCommand,
};

// ═══════════════════════════════════════════════════════════════
// Command Parsing & Dispatch
// ═══════════════════════════════════════════════════════════════

/**
 * Parse "/command args" format from message text.
 * Returns null if text is not a slash command.
 */
export function parseCommand(
  text: string,
): { name: string; args: string } | null {
  if (!text.startsWith("/")) return null;
  const trimmed = text.trim();
  const spaceIdx = trimmed.indexOf(" ");
  if (spaceIdx === -1) {
    return { name: trimmed.toLowerCase(), args: "" };
  }
  return {
    name: trimmed.slice(0, spaceIdx).toLowerCase(),
    args: trimmed.slice(spaceIdx + 1).trim(),
  };
}

/**
 * Check whether a command name is registered.
 */
export function isRegisteredCommand(name: string): boolean {
  return name in COMMAND_REGISTRY;
}

/**
 * Dispatch a parsed command to its handler.
 * Returns the formatted response string, or null if not a registered command.
 * If the handler returns void, returns null.
 */
export async function dispatchCommand(
  ctx: CommandContext,
): Promise<string | null> {
  const parsed = parseCommand(ctx.args);
  if (!parsed) return null;

  const command = COMMAND_REGISTRY[parsed.name];
  if (!command) return null;

  const commandCtx: CommandContext = {
    ...ctx,
    args: parsed.args,
  };

  try {
    const result = await command.handler(commandCtx);
    return typeof result === "string" ? result : null;
  } catch (err: any) {
    const message = err instanceof Error ? err.message : String(err);
    return formatError("命令失败", message);
  }
}

// ═══════════════════════════════════════════════════════════════
// Help Text
// ═══════════════════════════════════════════════════════════════

/**
 * Generate help text listing all registered commands.
 * Uses WhatsApp formatting: *bold*, _italic_, ```code```.
 */
export function formatHelp(): string {
  const groups: [string, string[]][] = [
    [
      "💬 *对话*",
      [
        "/compact",
        "/think",
        "/model",
        "/history",
        "/search",
        "/export",
        "/abort",
      ],
    ],
    ["📂 *项目 & Git*", ["/projects", "/git", "/diff", "/commit"]],
    ["🔧 *工具 & 扩展*", ["/tools", "/mcp", "/skills", "/plugins"]],
    ["⚙️ *自动化*", ["/task", "/tasks", "/task_stop", "/cron"]],
    ["📋 *Spec*", ["/spec"]],
    ["🚪 *网关*", ["/gateway"]],
    ["🔌 *会话*", ["/help", "/status", "/start", "/clear"]],
  ];

  let out = "📖 *BaoClaw 命令列表*\n\n";

  for (const [group, cmds] of groups) {
    out += `${group}\n`;
    for (const cmd of cmds) {
      const def = COMMAND_REGISTRY[cmd];
      if (def) {
        const usage = def.usage ? ` \`${def.usage}\`` : "";
        out += `  ${cmd} — ${def.description}${usage}\n`;
      }
    }
    out += "\n";
  }

  out += "_发送任意非命令消息即可与 AI 对话_";

  return out;
}
