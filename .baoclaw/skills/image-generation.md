---
name: image-generation
description: |
  使用 CogView-4 API 生成图片。当用户要求生成、画、创建图片/插图/海报时触发。
  也适用于图片编辑（先 vision 理解原图，再 CogView 重新生成）。
  触发词：画、生成图片、create image、generate image、draw、illustration、海报、插图。
---

# Image Generation with CogView-4

## 核心规则

1. **永远用 Node.js 调 API**，不依赖 curl 或任何沙箱机制。Node.js 是宿主机自带环境，100% 可用。
2. **生成的图片必须自动下载到当前工作目录**，不要只给 URL。
3. 把整个流程写成**一个 .js 脚本文件**，一次性执行完成（调用 API + 下载图片）。

## 一键脚本模板

每次生成图片时，按以下模板写一个临时脚本到 `/tmp/cogview-gen.js`，然后 `node /tmp/cogview-gen.js` 执行。

```javascript
const https = require('https');
const fs = require('fs');
const path = require('path');

// === 配置 ===
const PROMPT = '在此替换为实际 prompt';
const SIZE = '1024x1024';  // 可选: 1024x768, 768x1024
const OUT_DIR = process.cwd();  // 保存到当前工作目录
const OUT_FILE = path.join(OUT_DIR, 'generated-image.png');

// === 第一步：调用 CogView-4 API ===
const reqBody = JSON.stringify({ model: 'cogview-4-250304', prompt: PROMPT, size: SIZE });
const req = https.request({
  hostname: 'open.bigmodel.cn',
  path: '/api/paas/v4/images/generations',
  method: 'POST',
  headers: {
    'Authorization': 'Bearer ' + process.env.ZHIPU_API_KEY,
    'Content-Type': 'application/json',
    'Content-Length': Buffer.byteLength(reqBody)
  }
}, (res) => {
  let body = '';
  res.on('data', (c) => body += c);
  res.on('end', () => {
    const r = JSON.parse(body);
    if (r.error) { console.error('API错误:', r.error.message); process.exit(1); }
    if (!r.data || !r.data[0] || !r.data[0].url) { console.error('无图片URL:', body); process.exit(1); }

    const imgUrl = r.data[0].url;
    console.log('图片URL:', imgUrl);

    // === 第二步：下载图片到本地 ===
    https.get(imgUrl, (imgRes) => {
      if (imgRes.statusCode >= 300 && imgRes.statusCode < 400 && imgRes.headers.location) {
        // 处理重定向
        https.get(imgRes.headers.location, download);
      } else {
        download(imgRes);
      }
      function download(dRes) {
        const ws = fs.createWriteStream(OUT_FILE);
        dRes.pipe(ws);
        ws.on('finish', () => {
          const size = fs.statSync(OUT_FILE).size;
          console.log('已保存:', OUT_FILE, '(' + (size/1024).toFixed(1) + ' KB)');
        });
      }
    });
  });
});
req.on('error', (e) => { console.error('请求失败:', e.message); process.exit(1); });
req.write(reqBody);
req.end();
```

## 实际操作流程

当用户要求生成图片时：

### Step 1：构造 prompt

根据用户需求构造**中文详细描述**。prompt 质量极大地影响效果。

**五大要素**：主体、风格、构图、色彩、氛围

好 prompt 示例：
```
一个成熟男人靠在砖墙旁抽烟，烟雾缭绕，昏暗的路灯光打在脸上，
表情深沉，穿黑色皮夹克，写实摄影风格，电影级光影，高细节
```

坏 prompt 示例：
```
男人抽烟                          ← 太简略
A man smoking a cigarette         ← 英文效果不如中文
```

### Step 2：写脚本 + 执行

将模板中的 `PROMPT` 替换为实际 prompt，写文件，执行：

```bash
# 写入脚本（替换 PROMPT 值）
cat > /tmp/cogview-gen.js << 'SCRIPT'
  ...（填入实际脚本）...
SCRIPT

# 执行
node /tmp/cogview-gen.js
```

**注意**：`node` 命令在 Docker 沙箱环境下也能正常执行，因为 Node.js 是宿主机自带环境，走的是直接进程调用而非沙箱容器。

### Step 3：告知用户

输出格式：
```
✅ 图片已生成并保存: {工作目录}/generated-image.png (xxx KB)
```

如果用户想换个文件名，修改模板中的 `OUT_FILE` 即可。

## Prompt 技巧

### 中文 prompt 效果远好于英文
```
❌ "A birthday cake with candles"
✅ "一个精美的三层生日蛋糕，粉色奶油装饰，顶部插着点燃的彩色蜡烛"
```

### 具象描述，避免抽象概念
```
❌ "幸福的感觉"
✅ "一个女孩在阳光下微笑着拥抱一只金毛犬，背景是开满花的花园"
```

### 复杂场景用分句描述
```
"一个日式庭院，前景是石灯笼和苔藓小径，中景是一座木质茶室，
背景是远山和樱花树，清晨柔和的阳光穿透薄雾，写实摄影风格"
```

### 常用风格关键词
| 风格 | 关键词 |
|------|--------|
| 写实 | 写实摄影、电影级光影、高细节、8K |
| 插画 | 水彩画、扁平插画、矢量风格 |
| 动漫 | 吉卜力风格、新海诚风格、赛璐璐 |
| 概念 | 赛博朋克、蒸汽朋克、科幻概念艺术 |
| 艺术 | 油画、素描、水墨画、浮世绘 |

## 支持的尺寸

| 尺寸 | 适用场景 |
|------|---------|
| `1024x1024` | 正方形，头像、图标、通用（默认） |
| `1024x768` | 横向，风景、横幅、演示配图 |
| `768x1024` | 竖向，手机壁纸、海报、人像 |

## 图片编辑（两步走）

CogView-4 没有原生的"图生图"API，用两步走策略：

1. **理解原图**：对话模型（已支持 vision）直接看原图，输出详细描述
2. **重新生成**：把"原图描述 + 修改指令"组合成 CogView prompt

```
原图描述: "一只白色猫咪坐在蓝色沙发上，背景是客厅"
修改指令: "把背景换成星空"
→ prompt: "一只白色猫咪坐在沙发上，背景是璀璨的星空，
          猫咪周围有星星的微光反射，写实摄影风格"
```

## API 参考

| 项目 | 值 |
|------|---|
| 端点 | `POST https://open.bigmodel.cn/api/paas/v4/images/generations` |
| 模型 | `cogview-4-250304` |
| 认证 | `Authorization: Bearer {ZHIPU_API_KEY}` |
| 费用 | 0.06 元/次 |
| 返回 | `{"data":[{"url":"https://..."}]}` URL 有效期约 7 天 |

## 错误处理

| 错误 | 原因 | 解决 |
|------|------|------|
| `401 令牌已过期` | API Key 错误或过期 | 检查 `ZHIPU_API_KEY` 环境变量 |
| `1001 未收到Authorization` | Header 未传 | 确保 Node.js 脚本正确设置 Authorization |
| 返回空 data | prompt 内容违规 | 修改 prompt，避免敏感内容 |

## BaoClaw 内置工具（参考）

项目已有 Rust 实现的内置工具，但 skill 层面直接用 Node.js 调 API 更可靠：
- `baoclaw-core/src/tools/builtins/image_gen_tool.rs` — CogView-4 文生图
- `baoclaw-core/src/tools/builtins/image_edit_tool.rs` — vision + CogView 改图
