use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::trait_def::*;

/// Image editing tool — combines vision understanding with CogView-4 regeneration
pub struct ImageEditTool {
    http_client: reqwest::Client,
}

impl Default for ImageEditTool {
    fn default() -> Self {
        Self::new()
    }
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
        _context: &ToolContext,
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

        let image_response = self.http_client
            .get(image_url)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to download image: {}", e)))?;

        let image_bytes = image_response.bytes().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image bytes: {}", e)))?;

        let base64_data = base64_encode_bytes(&image_bytes);

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

fn base64_encode_bytes(bytes: &bytes::Bytes) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
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
