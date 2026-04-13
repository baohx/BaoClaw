use async_trait::async_trait;
use serde_json::{json, Value};

use super::path_utils::resolve_and_validate_path;
use crate::tools::trait_def::*;

/// Jupyter Notebook editing tool — operates on .ipynb JSON structure
pub struct NotebookEditTool;

impl NotebookEditTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "NotebookEditTool"
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["NotebookEdit"]
    }

    fn input_schema(&self) -> JsonSchema {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "notebook_path": {
                    "type": "string",
                    "description": ".ipynb file path"
                },
                "operation": {
                    "type": "string",
                    "enum": ["insert_cell", "replace_cell", "delete_cell", "move_cell"]
                },
                "cell_index": { "type": "integer" },
                "cell_type": { "type": "string", "enum": ["code", "markdown"] },
                "source": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Cell content lines"
                },
                "to_index": { "type": "integer", "description": "Target index for move_cell" }
            })),
            required: Some(vec![
                "notebook_path".to_string(),
                "operation".to_string(),
            ]),
            description: Some("Edit Jupyter notebook cells".to_string()),
        }
    }

    fn prompt(&self) -> String {
        "Edit Jupyter notebook cells. Supports insert, replace, delete, and move operations."
            .to_string()
    }

    async fn call(
        &self,
        input: Value,
        context: &ToolContext,
        _progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError> {
        let notebook_path_str = input
            .get("notebook_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'notebook_path'".to_string()))?;

        let resolved = resolve_and_validate_path(notebook_path_str, &context.cwd, &[])
            .map_err(|e| ToolError::ExecutionFailed(e))?;

        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing 'operation'".to_string()))?;

        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read notebook: {}", e)))?;

        let mut notebook: Value = serde_json::from_str(&content)
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid notebook JSON: {}", e)))?;

        let cell_index = input
            .get("cell_index")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        match operation {
            "insert_cell" => {
                let idx = cell_index
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'cell_index'".into()))?;
                let cell_type = input
                    .get("cell_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("code");
                let source = input
                    .get("source")
                    .cloned()
                    .unwrap_or(json!([]));
                edit_notebook_insert(&mut notebook, idx, cell_type, source)?;
            }
            "replace_cell" => {
                let idx = cell_index
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'cell_index'".into()))?;
                let source = input
                    .get("source")
                    .cloned()
                    .unwrap_or(json!([]));
                edit_notebook_replace(&mut notebook, idx, source)?;
            }
            "delete_cell" => {
                let idx = cell_index
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'cell_index'".into()))?;
                edit_notebook_delete(&mut notebook, idx)?;
            }
            "move_cell" => {
                let from = cell_index
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'cell_index' (from)".into()))?;
                let to = input
                    .get("to_index")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .ok_or_else(|| ToolError::ExecutionFailed("Missing 'to_index'".into()))?;
                edit_notebook_move(&mut notebook, from, to)?;
            }
            other => {
                return Err(ToolError::ExecutionFailed(format!(
                    "Unknown operation: {}",
                    other
                )));
            }
        }

        let output = serde_json::to_string_pretty(&notebook)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to serialize: {}", e)))?;

        tokio::fs::write(&resolved, output)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write notebook: {}", e)))?;

        let cell_count = notebook["cells"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);

        Ok(ToolResult {
            data: json!({
                "success": true,
                "operation": operation,
                "cell_count": cell_count,
            }),
            is_error: false,
        })
    }
}

fn get_cells_mut(notebook: &mut Value) -> Result<&mut Vec<Value>, ToolError> {
    notebook["cells"]
        .as_array_mut()
        .ok_or_else(|| {
            ToolError::ExecutionFailed("Invalid notebook: missing cells array".to_string())
        })
}

fn edit_notebook_insert(
    notebook: &mut Value,
    cell_index: usize,
    cell_type: &str,
    source: Value,
) -> Result<(), ToolError> {
    let cells = get_cells_mut(notebook)?;
    if cell_index > cells.len() {
        return Err(ToolError::ExecutionFailed(format!(
            "cell_index {} out of range (0..={})",
            cell_index,
            cells.len()
        )));
    }
    let mut new_cell = json!({
        "cell_type": cell_type,
        "source": source,
        "metadata": {},
    });
    if cell_type == "code" {
        new_cell["outputs"] = json!([]);
        new_cell["execution_count"] = Value::Null;
    }
    cells.insert(cell_index, new_cell);
    Ok(())
}

fn edit_notebook_replace(
    notebook: &mut Value,
    cell_index: usize,
    source: Value,
) -> Result<(), ToolError> {
    let cells = get_cells_mut(notebook)?;
    let len = cells.len();
    let cell = cells.get_mut(cell_index).ok_or_else(|| {
        ToolError::ExecutionFailed(format!(
            "cell_index {} out of range (0..{})",
            cell_index, len
        ))
    })?;
    cell["source"] = source;
    Ok(())
}

fn edit_notebook_delete(notebook: &mut Value, cell_index: usize) -> Result<(), ToolError> {
    let cells = get_cells_mut(notebook)?;
    if cell_index >= cells.len() {
        return Err(ToolError::ExecutionFailed(format!(
            "cell_index {} out of range (0..{})",
            cell_index,
            cells.len()
        )));
    }
    cells.remove(cell_index);
    Ok(())
}

fn edit_notebook_move(
    notebook: &mut Value,
    from_index: usize,
    to_index: usize,
) -> Result<(), ToolError> {
    let cells = get_cells_mut(notebook)?;
    if from_index >= cells.len() {
        return Err(ToolError::ExecutionFailed(format!(
            "from_index {} out of range (0..{})",
            from_index,
            cells.len()
        )));
    }
    if to_index > cells.len() {
        return Err(ToolError::ExecutionFailed(format!(
            "to_index {} out of range (0..={})",
            to_index,
            cells.len()
        )));
    }
    let cell = cells.remove(from_index);
    let insert_at = if to_index > from_index {
        to_index - 1
    } else {
        to_index
    };
    cells.insert(insert_at, cell);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_notebook() -> Value {
        json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {},
            "cells": [
                {
                    "cell_type": "code",
                    "source": ["print('hello')"],
                    "metadata": {},
                    "outputs": [],
                    "execution_count": null
                },
                {
                    "cell_type": "markdown",
                    "source": ["# Title"],
                    "metadata": {}
                }
            ]
        })
    }

    #[test]
    fn test_tool_name() {
        let tool = NotebookEditTool::new();
        assert_eq!(tool.name(), "NotebookEditTool");
    }

    #[test]
    fn test_tool_aliases() {
        let tool = NotebookEditTool::new();
        assert_eq!(tool.aliases(), vec!["NotebookEdit"]);
    }

    #[test]
    fn test_insert_cell_code() {
        let mut nb = sample_notebook();
        edit_notebook_insert(&mut nb, 1, "code", json!(["x = 1"])).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[1]["cell_type"], "code");
        assert_eq!(cells[1]["source"], json!(["x = 1"]));
        assert_eq!(cells[1]["outputs"], json!([]));
    }

    #[test]
    fn test_insert_cell_markdown() {
        let mut nb = sample_notebook();
        edit_notebook_insert(&mut nb, 0, "markdown", json!(["## Subtitle"])).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0]["cell_type"], "markdown");
        assert!(cells[0].get("outputs").is_none());
    }

    #[test]
    fn test_insert_cell_at_end() {
        let mut nb = sample_notebook();
        edit_notebook_insert(&mut nb, 2, "code", json!(["y = 2"])).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[2]["source"], json!(["y = 2"]));
    }

    #[test]
    fn test_insert_cell_out_of_bounds() {
        let mut nb = sample_notebook();
        let result = edit_notebook_insert(&mut nb, 10, "code", json!([]));
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_cell() {
        let mut nb = sample_notebook();
        edit_notebook_replace(&mut nb, 0, json!(["print('world')"])).unwrap();
        assert_eq!(nb["cells"][0]["source"], json!(["print('world')"]));
    }

    #[test]
    fn test_replace_cell_out_of_bounds() {
        let mut nb = sample_notebook();
        let result = edit_notebook_replace(&mut nb, 5, json!(["x"]));
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_cell() {
        let mut nb = sample_notebook();
        edit_notebook_delete(&mut nb, 0).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0]["cell_type"], "markdown");
    }

    #[test]
    fn test_delete_cell_out_of_bounds() {
        let mut nb = sample_notebook();
        let result = edit_notebook_delete(&mut nb, 5);
        assert!(result.is_err());
    }

    #[test]
    fn test_move_cell_forward() {
        let mut nb = sample_notebook();
        // Move cell 0 to position 2 (after last)
        edit_notebook_move(&mut nb, 0, 2).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells[0]["cell_type"], "markdown");
        assert_eq!(cells[1]["cell_type"], "code");
    }

    #[test]
    fn test_move_cell_backward() {
        let mut nb = sample_notebook();
        edit_notebook_move(&mut nb, 1, 0).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells[0]["cell_type"], "markdown");
        assert_eq!(cells[1]["cell_type"], "code");
    }

    #[test]
    fn test_move_cell_from_out_of_bounds() {
        let mut nb = sample_notebook();
        let result = edit_notebook_move(&mut nb, 10, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_move_cell_to_out_of_bounds() {
        let mut nb = sample_notebook();
        let result = edit_notebook_move(&mut nb, 0, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_notebook_no_cells() {
        let mut nb = json!({"metadata": {}});
        let result = edit_notebook_insert(&mut nb, 0, "code", json!([]));
        assert!(result.is_err());
    }
}
