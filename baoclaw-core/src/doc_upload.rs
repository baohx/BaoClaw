//! Document upload module for building attachment blocks from PDF/DOCX files.
//!
//! Supports:
//! - PDF: reads raw bytes → base64 encodes → builds document attachment block
//! - DOCX: extracts text from paragraphs → builds text attachment block
//!
//! Validates file size (max 10MB) and format (only pdf/docx).

use base64::Engine;
use serde_json::{json, Value};
use std::path::Path;
use thiserror::Error;

/// Maximum allowed file size: 10 MB
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Errors that can occur during document processing.
#[derive(Debug, Error)]
pub enum DocError {
    #[error("文件不存在: {0}")]
    FileNotFound(String),

    #[error("文件过大，最大支持 10MB")]
    FileTooLarge,

    #[error("不支持的文件格式: {0}")]
    UnsupportedFormat(String),

    #[error("文档解析失败: {0}")]
    ParseError(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// Validates that the file size does not exceed the 10MB limit.
///
/// Returns `Ok(())` if size is within limit, `Err(DocError::FileTooLarge)` otherwise.
pub fn validate_file_size(size: u64) -> Result<(), DocError> {
    if size > MAX_FILE_SIZE {
        return Err(DocError::FileTooLarge);
    }
    Ok(())
}

/// Validates that the file extension is supported (pdf or docx).
///
/// Returns `Ok(())` if supported, `Err(DocError::UnsupportedFormat)` otherwise.
pub fn validate_format(path: &Path) -> Result<(), DocError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdf" | "docx" => Ok(()),
        _ => Err(DocError::UnsupportedFormat(ext)),
    }
}

/// Builds an attachment block from a file path.
///
/// - PDF files: reads bytes → base64 encodes → returns document block
/// - DOCX files: extracts text from paragraphs → returns text block
///
/// Validates file existence, size (≤10MB), and format (pdf/docx) before processing.
pub fn build_attachment_from_file(path: &Path) -> Result<Value, DocError> {
    // Check file exists
    if !path.exists() {
        return Err(DocError::FileNotFound(path.display().to_string()));
    }

    // Validate format
    validate_format(path)?;

    // Validate file size
    let metadata = std::fs::metadata(path)?;
    validate_file_size(metadata.len())?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdf" => build_pdf_attachment(path),
        "docx" => build_docx_attachment(path),
        _ => Err(DocError::UnsupportedFormat(ext)),
    }
}

/// Builds a PDF document attachment block.
///
/// Reads the file bytes and base64 encodes them into the Anthropic document block format.
fn build_pdf_attachment(path: &Path) -> Result<Value, DocError> {
    let bytes = std::fs::read(path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);

    Ok(json!({
        "type": "document",
        "source": {
            "type": "base64",
            "media_type": "application/pdf",
            "data": encoded
        }
    }))
}

/// Builds a text attachment block from a DOCX file.
///
/// Uses docx-rs to parse the document and extract text from all paragraphs.
fn build_docx_attachment(path: &Path) -> Result<Value, DocError> {
    let bytes = std::fs::read(path)?;
    let text = extract_text_from_docx(&bytes)?;

    Ok(json!({
        "type": "text",
        "text": text
    }))
}

/// Extracts text content from DOCX bytes by iterating through paragraphs and runs.
fn extract_text_from_docx(bytes: &[u8]) -> Result<String, DocError> {
    let docx = docx_rs::read_docx(bytes).map_err(|e| DocError::ParseError(format!("{}", e)))?;

    let mut text_parts: Vec<String> = Vec::new();

    for child in docx.document.children {
        if let docx_rs::DocumentChild::Paragraph(paragraph) = child {
            let mut para_text = String::new();
            for content in &paragraph.children {
                if let docx_rs::ParagraphChild::Run(run) = content {
                    for run_child in &run.children {
                        if let docx_rs::RunChild::Text(text) = run_child {
                            para_text.push_str(&text.text);
                        }
                    }
                }
            }
            if !para_text.is_empty() {
                text_parts.push(para_text);
            }
        }
    }

    let result = text_parts.join("\n");
    Ok(result)
}

/// Builds a PDF attachment block directly from bytes (useful for testing).
pub fn build_pdf_attachment_from_bytes(bytes: &[u8]) -> Value {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    json!({
        "type": "document",
        "source": {
            "type": "base64",
            "media_type": "application/pdf",
            "data": encoded
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_file_size_within_limit() {
        assert!(validate_file_size(0).is_ok());
        assert!(validate_file_size(MAX_FILE_SIZE).is_ok());
    }

    #[test]
    fn test_validate_file_size_exceeds_limit() {
        assert!(validate_file_size(MAX_FILE_SIZE + 1).is_err());
    }

    #[test]
    fn test_validate_format_pdf() {
        let path = Path::new("test.pdf");
        assert!(validate_format(path).is_ok());
    }

    #[test]
    fn test_validate_format_docx() {
        let path = Path::new("test.docx");
        assert!(validate_format(path).is_ok());
    }

    #[test]
    fn test_validate_format_unsupported() {
        let path = Path::new("test.txt");
        assert!(validate_format(path).is_err());

        let path = Path::new("test.jpg");
        assert!(validate_format(path).is_err());
    }

    #[test]
    fn test_validate_format_no_extension() {
        let path = Path::new("noext");
        assert!(validate_format(path).is_err());
    }

    #[test]
    fn test_build_pdf_attachment() {
        let mut tmp = NamedTempFile::with_suffix(".pdf").unwrap();
        let content = b"fake pdf content";
        tmp.write_all(content).unwrap();

        let result = build_attachment_from_file(tmp.path()).unwrap();
        assert_eq!(result["type"], "document");
        assert_eq!(result["source"]["type"], "base64");
        assert_eq!(result["source"]["media_type"], "application/pdf");

        // Verify base64 roundtrip
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(result["source"]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(decoded, content);
    }

    #[test]
    fn test_file_not_found() {
        let path = Path::new("/nonexistent/file.pdf");
        let result = build_attachment_from_file(path);
        assert!(matches!(result, Err(DocError::FileNotFound(_))));
    }

    #[test]
    fn test_file_too_large() {
        let mut tmp = NamedTempFile::with_suffix(".pdf").unwrap();
        // Write just over 10MB
        let data = vec![0u8; (MAX_FILE_SIZE + 1) as usize];
        tmp.write_all(&data).unwrap();

        let result = build_attachment_from_file(tmp.path());
        assert!(matches!(result, Err(DocError::FileTooLarge)));
    }

    #[test]
    fn test_unsupported_format_via_build() {
        let tmp = NamedTempFile::with_suffix(".txt").unwrap();
        let result = build_attachment_from_file(tmp.path());
        assert!(matches!(result, Err(DocError::UnsupportedFormat(_))));
    }

    #[test]
    fn test_build_pdf_attachment_from_bytes() {
        let bytes = b"hello world";
        let block = build_pdf_attachment_from_bytes(bytes);
        assert_eq!(block["type"], "document");
        assert_eq!(block["source"]["media_type"], "application/pdf");

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(block["source"]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn test_build_docx_attachment() {
        // Create a minimal DOCX file using docx-rs
        let docx = docx_rs::Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Hello World")),
            )
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Second paragraph")),
            );

        let tmp = NamedTempFile::with_suffix(".docx").unwrap();
        let file = std::fs::File::create(tmp.path()).unwrap();
        docx.build().pack(file).unwrap();

        let result = build_attachment_from_file(tmp.path()).unwrap();
        assert_eq!(result["type"], "text");
        let text = result["text"].as_str().unwrap();
        assert!(text.contains("Hello World"));
        assert!(text.contains("Second paragraph"));
    }
}
