# Multi-Terminal Image Capability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable image send/receive/generate/edit across Terminal TUI, Telegram, and Web clients.

**Architecture:** 
- New Rust core builtin tools (`image_generate`, `image_edit`) calling CogView-4 API (GLM OpenAI-compatible endpoint)
- Client-side changes: TUI handles paste/drag→base64→attachments, Telegram receives photos→base64→attachments + sendPhoto for output, Web adds upload button→base64→attachments + `<img>` display
- All clients reuse the existing `SubmitMessage { prompt, attachments }` IPC pipeline — no core protocol changes needed

**Tech Stack:** Rust (reqwest, serde, base64), TypeScript (node-telegram-bot-api, ws), HTML/JS

---

## File Structure

### New Files
| File | Responsibility |
|------|---------------|
| `baoclaw-core/src/tools/builtins/image_gen_tool.rs` | CogView-4 image generation tool |
| `baoclaw-core/src/tools/builtins/image_edit_tool.rs` | Image editing tool (vision + regenerate) |

### Modified Files
| File | Change |
|------|--------|
| `baoclaw-core/src/tools/builtins/mod.rs` | Register new tool modules |
| `baoclaw-core/src/main.rs` | Instantiate and register new tools |
| `baoclaw-telegram/src/gateway.ts` | Photo receive→attachments, tool result→sendPhoto |
| `baoclaw-web/src/server.ts` | Handle image upload action |
| `baoclaw-web/public/app.js` | Upload button, preview, image display |
| `baoclaw-web/public/index.html` | Upload button HTML, image preview area |

---

## Task 1: Rust Core — Image Generate Tool

**Files:**
- Create: `baoclaw-core/src/tools/builtins/image_gen_tool.rs`
- Modify: `baoclaw-core/src/tools/builtins/mod.rs`
- Modify: `baoclaw-core/src/main.rs`

- [ ] **Step 1: Create `image_gen_tool.rs`**

```rust
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// Image generation tool — calls CogView-4 via GLM OpenAI-compatible API
pub struct ImageGenTool {
    http_client: reqwest::Client,
}

impl ImageGenTool {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for ImageGenTool {
    fn name(&self) -> &str { "ImageGenerator" }

    fn aliases(&self) -> Vec<&str> { vec!["GenerateImage", "CreateImage"] }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "prompt": {
                    "type": "string",
                    "description": "Image generation prompt. Supports Chinese and English. Be descriptive about style, content, composition."
                },
                "size": {
                    "type": "string",
                    "description": "Image size, e.g. '1024x1024', '1024x768', '768x1024'. Default: '1024x1024'"
                }
            })),
            required: Some(vec!["prompt".to_string()]),
            description: Some("Generate an image from a text description using CogView-4. Use when the user asks to create, draw, or generate an image/picture/illustration.".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool { false }
    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }

    fn max_result_size_chars(&self) -> usize { 10_000_000 } // 10MB for base64 images

    fn prompt(&self) -> String {
        "Generate images from text descriptions using CogView-4. Use when the user asks to create, draw, generate, or design an image, picture, illustration, or visual. The tool supports Chinese and English prompts. Returns a base64-encoded PNG image.".to_string()
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        // Read API config from environment (same as main LLM client)
        let api_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .map_err(|_| ToolError::ExecutionFailed(
                "No API key found. Set OPENAI_API_KEY or ANTHROPIC_API_KEY.".to_string()
            ))?;

        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".to_string());

        let prompt = input.get("prompt").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'prompt' field".to_string()))?;

        let size = input.get("size").and_then(|v| v.as_str())
            .unwrap_or("1024x1024");

        let model = std::env::var("IMAGE_GEN_MODEL")
            .unwrap_or_else(|_| "cogview-4-250304".to_string());

        // Call CogView-4 API (OpenAI-compatible /images/generations endpoint)
        let url = format!("{}/images/generations", base_url.trim_end_matches('/'));
        let body = json!({
            "model": model,
            "prompt": prompt,
            "size": size,
        });

        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Ok(ToolResult {
                data: json!({
                    "error": format!("CogView API error ({}): {}", status, body),
                    "is_error": true,
                }),
                is_error: true,
            });
        }

        let resp_json: Value = response.json().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse API response: {}", e)))?;

        // Extract image URL from response: { "data": [{ "url": "..." }] }
        let image_url = resp_json
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("url"))
            .and_then(|u| u.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed(
                format!("Unexpected API response format: {}", resp_json)
            ))?;

        // Download the image
        let image_response = self.http_client
            .get(image_url)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to download image: {}", e)))?;

        let image_bytes = image_response.bytes().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image bytes: {}", e)))?;

        // Encode as base64
        let base64_data = base64_encode(&image_bytes);

        // Detect media type from bytes (PNG magic: 89 50 4E 47, JPEG magic: FF D8 FF)
        let media_type = if image_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            "image/png"
        } else if image_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "image/jpeg"
        } else {
            "image/png" // default
        };

        Ok(ToolResult {
            data: json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": base64_data,
                },
                "prompt": prompt,
                "size": size,
            }),
            is_error: false,
        })
    }
}

fn base64_encode(bytes: &bytes::Bytes) -> String {
    use std::fmt::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((bytes.len() * 4 + 2) / 3);
    let chunks = bytes.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}
```

- [ ] **Step 2: Register in `builtins/mod.rs`**

Add at the end of the file:
```rust
pub mod image_gen_tool;
pub use image_gen_tool::ImageGenTool;
```

- [ ] **Step 3: Register in `main.rs` tool instantiation**

Find where tools are created (search for `WebSearchTool::new()` or similar). Add:
```rust
ImageGenTool::new(),
```

- [ ] **Step 4: Build and verify compilation**

Run: `cd baoclaw-core && cargo build 2>&1 | tail -20`
Expected: Successful compilation (warnings OK, errors must be fixed)

- [ ] **Step 5: Commit**

```bash
git add baoclaw-core/src/tools/builtins/image_gen_tool.rs baoclaw-core/src/tools/builtins/mod.rs baoclaw-core/src/main.rs
git commit -m "feat: add ImageGenTool — CogView-4 image generation via GLM API"
```

---

## Task 2: Rust Core — Image Edit Tool

**Files:**
- Create: `baoclaw-core/src/tools/builtins/image_edit_tool.rs`
- Modify: `baoclaw-core/src/tools/builtins/mod.rs`
- Modify: `baoclaw-core/src/main.rs`

- [ ] **Step 1: Create `image_edit_tool.rs`**

This tool takes an edit instruction + an image description (already provided by the AI's vision analysis of the original image) and generates a new image via CogView-4.

```rust
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// Image editing tool — combines vision understanding with CogView-4 regeneration
pub struct ImageEditTool {
    http_client: reqwest::Client,
}

impl ImageEditTool {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for ImageEditTool {
    fn name(&self) -> &str { "ImageEditor" }

    fn aliases(&self) -> Vec<&str> { vec!["EditImage", "ModifyImage"] }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "instruction": {
                    "type": "string",
                    "description": "What to change in the image. E.g. 'change background to starry night', 'make it look like watercolor painting', 'add a red hat to the cat'"
                },
                "image_description": {
                    "type": "string",
                    "description": "Detailed description of the current image content (from your vision analysis). Used as base for the regeneration prompt."
                },
                "size": {
                    "type": "string",
                    "description": "Output image size. Default: '1024x1024'"
                }
            })),
            required: Some(vec!["instruction".to_string(), "image_description".to_string()]),
            description: Some("Edit/modify an image based on the user's instruction. The AI first analyzes the original image, then uses this tool to regenerate with modifications.".to_string()),
        }
    }

    fn is_read_only(&self, _input: &Value) -> bool { false }
    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }

    fn max_result_size_chars(&self) -> usize { 10_000_000 }

    fn prompt(&self) -> String {
        "Edit or modify an existing image. You must first analyze the original image using your vision capability, then call this tool with the edit instruction and a description of the original image content. The tool will regenerate the image with the requested changes.".to_string()
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .map_err(|_| ToolError::ExecutionFailed(
                "No API key found.".to_string()
            ))?;

        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".to_string());

        let instruction = input.get("instruction").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'instruction' field".to_string()))?;

        let image_description = input.get("image_description").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'image_description' field".to_string()))?;

        let size = input.get("size").and_then(|v| v.as_str())
            .unwrap_or("1024x1024");

        let model = std::env::var("IMAGE_GEN_MODEL")
            .unwrap_or_else(|_| "cogview-4-250304".to_string());

        // Combine description + edit instruction into a single generation prompt
        let combined_prompt = format!(
            "Based on this scene: {}. Apply these changes: {}",
            image_description, instruction
        );

        // Same CogView-4 API call as ImageGenTool
        let url = format!("{}/images/generations", base_url.trim_end_matches('/'));
        let body = json!({
            "model": model,
            "prompt": combined_prompt,
            "size": size,
        });

        let response = self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Ok(ToolResult {
                data: json!({
                    "error": format!("CogView API error ({}): {}", status, body_text),
                }),
                is_error: true,
            });
        }

        let resp_json: Value = response.json().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse API response: {}", e)))?;

        let image_url = resp_json
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("url"))
            .and_then(|u| u.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed(
                format!("Unexpected API response format: {}", resp_json)
            ))?;

        // Download and base64 encode
        let image_response = self.http_client
            .get(image_url)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to download image: {}", e)))?;

        let image_bytes = image_response.bytes().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image bytes: {}", e)))?;

        let base64_data = {
            use std::fmt::Write;
            const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut result = String::with_capacity((image_bytes.len() * 4 + 2) / 3);
            for chunk in image_bytes.chunks(3) {
                let b0 = chunk[0] as u32;
                let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
                let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
                let triple = (b0 << 16) | (b1 << 8) | b2;
                result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
                result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
                if chunk.len() > 1 { result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char); } else { result.push('='); }
                if chunk.len() > 2 { result.push(CHARS[(triple & 0x3F) as usize] as char); } else { result.push('='); }
            }
            result
        };

        let media_type = if image_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            "image/png"
        } else if image_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "image/jpeg"
        } else {
            "image/png"
        };

        Ok(ToolResult {
            data: json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": base64_data,
                },
                "instruction": instruction,
                "size": size,
            }),
            is_error: false,
        })
    }
}
```

- [ ] **Step 2: Register in `builtins/mod.rs`**

Add:
```rust
pub mod image_edit_tool;
pub use image_edit_tool::ImageEditTool;
```

- [ ] **Step 3: Register in `main.rs` tool instantiation**

Add alongside ImageGenTool:
```rust
ImageEditTool::new(),
```

- [ ] **Step 4: Build and verify**

Run: `cd baoclaw-core && cargo build 2>&1 | tail -20`

- [ ] **Step 5: Commit**

```bash
git add baoclaw-core/src/tools/builtins/image_edit_tool.rs baoclaw-core/src/tools/builtins/mod.rs baoclaw-core/src/main.rs
git commit -m "feat: add ImageEditTool — edit images via vision + CogView-4 regeneration"
```

---

## Task 3: Telegram — Image Receive & Send Enhancement

**Files:**
- Modify: `baoclaw-telegram/src/gateway.ts`

The Telegram gateway already has partial image support. We need to ensure:
1. Received photos → `attachments` array in `submitMessage`
2. `ImageGenerator` / `ImageEditor` tool results → `sendPhoto` to Telegram

- [ ] **Step 1: Read current gateway.ts photo handling code**

Search for `bot.on('photo')` and `buildImageBlock` and `extractAndSendImages` to understand current state.

Key areas to check:
- Line ~400-450: `bot.on('photo')` handler — does it pass `attachments` to `submitMessage`?
- Line ~300-350: `extractAndSendImages` function — does it handle tool_result image blocks?
- The `submitMessage` call — does it include `attachments`?

- [ ] **Step 2: Fix photo→attachments pipeline**

In the `bot.on('photo')` handler, ensure the downloaded photo is passed as `attachments` to the IPC `submitMessage` call:

```typescript
// In bot.on('photo') handler, after downloading and base64 encoding:
const imageBlock = buildImageBlock(base64Data, mimeType);

// In the IPC submitMessage call:
ipc.request('submitMessage', {
    prompt: caption || '请分析这张图片',
    attachments: [imageBlock],
});
```

- [ ] **Step 3: Add tool result image extraction**

Ensure `extractAndSendImages` also handles images from `ImageGenerator`/`ImageEditor` tool results. The tool results come back as `tool_result` events with `content` containing image blocks:

```typescript
// In the stream event handler for tool_result:
// Check if the tool result contains an image block
if (toolResult.content) {
    for (const block of Array.isArray(toolResult.content) ? toolResult.content : [toolResult.content]) {
        if (block.type === 'image' && block.source?.data) {
            // Send as photo to Telegram
            const buffer = Buffer.from(block.source.data, 'base64');
            bot.sendPhoto(chatId, buffer, { caption: '🎨 Generated image' });
        }
    }
}
```

- [ ] **Step 4: Test with a real Telegram message**

Send a photo to the bot with caption "描述这张图片", verify:
- Bot receives the photo
- Photo is passed as attachment to daemon
- AI responds with description

- [ ] **Step 5: Commit**

```bash
git add baoclaw-telegram/src/gateway.ts
git commit -m "feat(telegram): enhance photo receive/send — attach to submitMessage, send generated images"
```

---

## Task 4: Web — Image Upload & Display

**Files:**
- Modify: `baoclaw-web/public/index.html`
- Modify: `baoclaw-web/public/app.js`
- Modify: `baoclaw-web/src/server.ts`

- [ ] **Step 1: Add upload button and preview area in `index.html`**

In the input area (before or next to the send button), add:

```html
<!-- Inside the input area, after the textarea -->
<button id="uploadBtn" title="上传图片" style="...">📎</button>
<input type="file" id="imageInput" accept="image/*" multiple hidden>
<div id="imagePreview" class="image-preview"></div>
```

- [ ] **Step 2: Add upload logic in `app.js`**

```javascript
// Image upload handling
const uploadBtn = document.getElementById('uploadBtn');
const imageInput = document.getElementById('imageInput');
const imagePreview = document.getElementById('imagePreview');
let pendingImages = []; // { name, base64, mediaType }

uploadBtn.addEventListener('click', () => imageInput.click());

imageInput.addEventListener('change', (e) => {
    const files = Array.from(e.target.files);
    files.forEach(file => {
        const reader = new FileReader();
        reader.onload = (ev) => {
            const base64 = ev.target.result.split(',')[1]; // strip data:image/...;base64,
            const mediaType = file.type || 'image/png';
            pendingImages.push({ name: file.name, base64, mediaType });
            renderImagePreview();
        };
        reader.readAsDataURL(file);
    });
    imageInput.value = ''; // reset for re-upload
});

function renderImagePreview() {
    imagePreview.innerHTML = pendingImages.map((img, i) => `
        <div class="preview-thumb">
            <img src="data:${img.mediaType};base64,${img.base64}" />
            <button onclick="removeImage(${i})" class="remove-btn">×</button>
            <span>${img.name}</span>
        </div>
    `).join('');
}

function removeImage(index) {
    pendingImages.splice(index, 1);
    renderImagePreview();
}
```

- [ ] **Step 3: Modify `submitMessage` in `app.js` to include attachments**

Find the `submit` action handler. Add image attachments:

```javascript
function submitMessage() {
    const text = inputEl.value.trim();
    if (!text && pendingImages.length === 0) return;

    const msg = { action: 'submit' };

    if (pendingImages.length > 0) {
        // Build attachments array
        const attachments = pendingImages.map(img => ({
            type: 'image',
            source: {
                type: 'base64',
                media_type: img.mediaType,
                data: img.base64,
            }
        }));
        msg.prompt = text || '请分析这些图片';
        msg.attachments = attachments;

        // Show uploaded images in chat as user message
        appendUserImages(pendingImages);
        pendingImages = [];
        imagePreview.innerHTML = '';
    } else {
        msg.prompt = text;
    }

    ws.send(JSON.stringify(msg));
    inputEl.value = '';
}

function appendUserImages(images) {
    const el = document.createElement('div');
    el.className = 'message user-message';
    el.innerHTML = images.map(img =>
        `<img src="data:${img.mediaType};base64,${img.base64}" class="user-upload-image" onclick="showImageModal(this.src)" />`
    ).join('');
    // append to current tab's message container
    getActiveMsgEl().appendChild(el);
}
```

- [ ] **Step 4: Modify `server.ts` to pass attachments**

In the WebSocket `message` handler for `submit` action, add attachments passthrough:

```typescript
// In the ws.on('message') handler, case 'submit':
case 'submit': {
    const params: any = { prompt: msg.prompt };
    if (msg.attachments && msg.attachments.length > 0) {
        params.attachments = msg.attachments;
    }
    const result = await ipc.request('submitMessage', params);
    // ... existing response handling
    break;
}
```

- [ ] **Step 5: Add CSS for image preview and display in `style.css`**

```css
.image-preview {
    display: flex;
    gap: 8px;
    padding: 8px 0;
    flex-wrap: wrap;
}
.preview-thumb {
    position: relative;
    display: inline-block;
}
.preview-thumb img {
    width: 80px;
    height: 80px;
    object-fit: cover;
    border-radius: 6px;
    border: 2px solid var(--border-color, #333);
}
.preview-thumb .remove-btn {
    position: absolute;
    top: -6px;
    right: -6px;
    background: #e74c3c;
    color: white;
    border: none;
    border-radius: 50%;
    width: 20px;
    height: 20px;
    cursor: pointer;
    font-size: 12px;
    line-height: 20px;
    text-align: center;
}
.user-upload-image {
    max-width: 300px;
    max-height: 200px;
    border-radius: 8px;
    cursor: pointer;
    margin: 4px;
}
```

- [ ] **Step 6: Verify AI-generated images display correctly**

The existing `addToolResult` function in `app.js` already handles `type: 'image'` content blocks with `data:mimeType;base64,` display. Verify this works end-to-end by asking the AI to generate an image.

- [ ] **Step 7: Commit**

```bash
git add baoclaw-web/public/index.html baoclaw-web/public/app.js baoclaw-web/src/server.ts baoclaw-web/public/style.css
git commit -m "feat(web): add image upload with preview and AI image display"
```

---

## Task 5: Terminal TUI — Image Paste & Display

**Files:**
- Modify: `ts-ipc/cli.ts` or the TUI input handler (wherever paste/key input is handled)

This is the most complex task due to terminal image display protocol variations. We use a pragmatic approach: save to file + display path.

- [ ] **Step 1: Detect pasted file paths in TUI input**

When the user drags a file into the terminal, most terminal emulators paste the file path (often quoted or escaped). In the input handler:

```typescript
// In the input handler, detect image file paths:
function detectImagePaste(input: string): { path: string; remaining: string } | null {
    // Terminal paste of dragged file: usually '/path/to/file.png' or "path with spaces.png"
    const match = input.match(/^["']?(\/[^\s"']+\.(png|jpg|jpeg|gif|webp|bmp))["']?\s*(.*)/i);
    if (match) {
        return { path: match[1], remaining: match[3] || '' };
    }
    return null;
}
```

- [ ] **Step 2: Convert image file to attachment**

```typescript
import * as fs from 'fs';
import * as path from 'path';

function imageFileToAttachment(filePath: string): { attachment: any; error?: string } {
    try {
        const ext = path.extname(filePath).toLowerCase();
        const mimeMap: Record<string, string> = {
            '.png': 'image/png',
            '.jpg': 'image/jpeg',
            '.jpeg': 'image/jpeg',
            '.gif': 'image/gif',
            '.webp': 'image/webp',
            '.bmp': 'image/bmp',
        };
        const mediaType = mimeMap[ext];
        if (!mediaType) return { attachment: null, error: `Unsupported image format: ${ext}` };

        const data = fs.readFileSync(filePath);
        const base64 = data.toString('base64');
        return {
            attachment: {
                type: 'image',
                source: { type: 'base64', media_type: mediaType, data: base64 },
            }
        };
    } catch (e) {
        return { attachment: null, error: `Failed to read image: ${e.message}` };
    }
}
```

- [ ] **Step 3: Modify submit flow to include attachments**

When the TUI submits a message, if image attachments are detected:

```typescript
// In the submit handler:
const imagePaste = detectImagePaste(userInput);
if (imagePaste && fs.existsSync(imagePaste.path)) {
    const { attachment, error } = imageFileToAttachment(imagePaste.path);
    if (attachment) {
        await client.request('submitMessage', {
            prompt: imagePaste.remaining || '请分析这张图片',
            attachments: [attachment],
        });
    }
} else {
    await client.request('submitMessage', { prompt: userInput });
}
```

- [ ] **Step 4: Handle AI-generated image display in terminal**

When receiving tool results containing images from `ImageGenerator`/`ImageEditor`:

```typescript
// In the stream handler for tool_result containing image data:
function handleImageResult(imageData: string, mediaType: string) {
    const dir = path.join(os.tmpdir(), 'baoclaw-images');
    fs.mkdirSync(dir, { recursive: true });
    const filename = `image-${Date.now()}.${mediaType.split('/')[1] || 'png'}`;
    const filepath = path.join(dir, filename);
    fs.writeFileSync(filepath, Buffer.from(imageData, 'base64'));

    // Try terminal image protocols (iTerm2 / Kitty / Sixel)
    const term = process.env.TERM_PROGRAM || '';
    if (term === 'iTerm.app') {
        // iTerm2 inline image protocol
        const esc = `\x1B]1337;File=inline=1;width=auto;height=auto:${Buffer.from(filepath).toString('base64')}\x07`;
        process.stdout.write(esc + '\n');
    } else if (term === 'kitty') {
        // Kitty image protocol (simplified)
        // Fall back to path display
        console.log(`\n📷 图片已保存: ${filepath}\n`);
    } else {
        console.log(`\n📷 图片已保存: ${filepath}\n`);
    }
}
```

- [ ] **Step 5: Commit**

```bash
git add ts-ipc/cli.ts
git commit -m "feat(tui): add image file paste/drag support and image display"
```

---

## Task 6: Integration Test

- [ ] **Step 1: Test Telegram image send → AI recognition**

Send a photo to the Telegram bot with caption "这张图片里有什么？"
Expected: AI responds with a description of the image content.

- [ ] **Step 2: Test Telegram image generation**

Send "帮我画一只可爱的小猫" to the Telegram bot.
Expected: AI calls ImageGenerator, bot sends back a photo.

- [ ] **Step 3: Test Web image upload → AI recognition**

Upload an image via the web interface with prompt "描述这张图".
Expected: Image shows in chat, AI responds with description.

- [ ] **Step 4: Test Web image generation**

Type "生成一张日落风景图" in the web interface.
Expected: AI generates image, displayed in the chat.

- [ ] **Step 5: Test TUI image paste**

Drag an image file into the terminal TUI.
Expected: Image is analyzed, AI responds with description.

- [ ] **Step 6: Test image editing flow**

Send a photo + "把背景换成星空".
Expected: AI analyzes image, calls ImageEditor, returns modified image.

- [ ] **Step 7: Final commit with version bump**

```bash
git add -A
git commit -m "feat: multi-terminal image capability — generate, edit, recognize (v2.2.0)"
git tag v2.2.0
```

---

## Environment Variables Reference

| Variable | Purpose | Default |
|----------|---------|---------|
| `OPENAI_API_KEY` | GLM API key (shared with LLM client) | — |
| `OPENAI_BASE_URL` | GLM API base URL | `https://open.bigmodel.cn/api/paas/v4` |
| `IMAGE_GEN_MODEL` | CogView model name | `cogview-4-250304` |

## Key Design Decisions

1. **No `base64` crate dependency** — manual base64 encode avoids adding a crate. If this causes issues, switch to `base64` crate.
2. **Image edit = vision + regenerate** — GLM has no native Img2Img API. Two-step approach: AI sees original → generates modified version via CogView.
3. **Terminal image display = file save + path** — iTerm2 protocol for compatible terminals, file path for all others. Avoids Sixel/Kitty complexity.
4. **API key reuse** — Same `OPENAI_API_KEY` and `OPENAI_BASE_URL` used by the LLM client, no separate config needed.
5. **10MB tool result limit** — Matches existing MCP screenshot tool limit for base64 images.
