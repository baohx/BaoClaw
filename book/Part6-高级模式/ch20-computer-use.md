# 第 20 章：Computer Use —— 桌面控制 Agent

## 20.1 从代码到桌面

传统 Agent 操作的是文件和命令行。Computer Use 让 Agent 能操作图形界面 —— 截图、识别 UI 元素、点击、输入文字。

## 20.2 BaoClaw 的 Computer Use 实现

BaoClaw 通过两种方式实现桌面控制：

### 方式一：MCP 工具（computer-control-mcp）

```json
// ~/.baoclaw/mcp.json
{
    "mcpServers": {
        "computer-control-mcp": {
            "command": "uvx",
            "args": ["computer-control-mcp@latest"]
        }
    }
}
```

提供 15 个工具：`take_screenshot`、`click_screen`、`type_text`、`move_mouse`、`press_keys`、`list_windows`、`activate_window` 等。

### 方式二：Skill + Bash（无 MCP 依赖）

```markdown
# desktop-launch-app Skill
通过 xdotool + 截图分析的方式操作桌面：
1. xdotool 获取屏幕信息
2. Python + PIL 截图并分析像素
3. xdotool 移动鼠标并点击
```

Skill 方式不需要额外的 MCP 服务器，但需要系统安装 `xdotool` 和 `python3`。

## 20.3 图片处理的挑战

桌面截图是 Computer Use 的核心，但也带来了独特的工程挑战：

### 挑战 1：图片数据太大

2560×1600 的 PNG 截图，base64 编码后约 5-10MB。直接存入对话历史会撑爆上下文窗口。

解决：`strip_base64_images` 在存入对话历史前移除图片数据。

### 挑战 2：MCP 返回的图片格式

MCP 工具返回 JSON 格式的图片：
```json
{"content": [{"type": "image", "data": "iVBOR...", "mimeType": "image/png"}]}
```

但这个 JSON 被序列化为字符串传输，且可能被 `truncate_if_needed` 截断导致 JSON 不完整。

解决：MCP 工具的 `max_result_size_chars` 设为 10MB。

### 挑战 3：Telegram 显示图片

Telegram 不能显示 base64 文本。需要：
1. 从 tool_result 中提取 base64 数据
2. 解码为二进制
3. 写入临时文件
4. 通过 `bot.sendPhoto(chatId, tmpFile)` 发送
5. 删除临时文件

```typescript
case 'tool_result':
    let outputObj = JSON.parse(tr.output); // 可能因截断失败
    for (const item of outputObj.content) {
        if (item.type === 'image') {
            const tmpFile = path.join(os.tmpdir(), `baoclaw-img-${Date.now()}.png`);
            fs.writeFileSync(tmpFile, Buffer.from(item.data, 'base64'));
            await bot.sendPhoto(chatId, tmpFile);
            fs.unlinkSync(tmpFile);
        }
    }
```

## 20.4 工作流示例

用户说"帮我打开微信"：

```
1. AI 调用 take_screenshot → 获取桌面截图
2. AI 分析截图，找到任务栏图标位置
3. AI 调用 click_screen(x, y) → 点击微信图标
4. AI 调用 take_screenshot → 验证微信是否打开
5. AI 回复"微信已打开"
```

整个过程是 ReAct 循环的自然延伸 —— 只是工具从"读写文件"变成了"截图点击"。

## 20.5 小结

Computer Use 是 Agent 能力的重大扩展。通过 MCP 协议接入桌面控制工具，Agent 可以操作任何图形界面应用。工程上的主要挑战在于大图片数据的处理和跨客户端的图片传输。
