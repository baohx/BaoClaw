use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use regex::Regex;

/// Spec 工作流类型
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpecWorkflow {
    RequirementsFirst,
    DesignFirst,
}

/// Spec 当前阶段
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpecPhase {
    Requirements,
    RequirementsComplete,
    Design,
    DesignComplete,
    Tasks,
    TasksComplete,
}

/// Spec 类型
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpecType {
    Feature,
    Bugfix,
}

/// Spec 配置（存储在 .config.json）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpecConfig {
    pub workflow: SpecWorkflow,
    pub phase: SpecPhase,
    pub spec_type: SpecType,
    pub created_at: String,
    pub updated_at: String,
}

/// Spec 摘要信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecSummary {
    pub feature_name: String,
    pub workflow: SpecWorkflow,
    pub phase: SpecPhase,
    pub spec_type: SpecType,
    pub task_progress: Option<TaskProgress>,
}

/// 任务进度统计
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskProgress {
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
}

/// 任务状态
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TaskStatus {
    NotStarted,
    InProgress,
    Completed,
}

/// 单个任务条目
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskItem {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub requirement_ref: Option<String>,
    pub children: Vec<TaskItem>,
}

/// 解析后的任务文档
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskDocument {
    pub preamble: String,
    pub tasks: Vec<TaskItem>,
    pub postamble: String,
}

/// Spec 引擎错误类型
#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("Spec not found: {0}")]
    NotFound(String),

    #[error("Spec already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid phase transition: {current:?} → {target:?}")]
    InvalidPhaseTransition { current: SpecPhase, target: SpecPhase },

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Phase document not available: {phase} (current phase: {current:?})")]
    PhaseNotAvailable { phase: String, current: SpecPhase },

    #[error("Parse error in tasks.md: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config parse error: {0}")]
    ConfigError(#[from] serde_json::Error),
}

// ─── TaskTracker ───────────────────────────────────────────────────────────────

/// Task parser/serializer for tasks.md files.
pub struct TaskTracker;

impl TaskTracker {
    /// Parse tasks.md content into a structured TaskDocument.
    pub fn parse(content: &str) -> Result<TaskDocument, SpecError> {
        let lines: Vec<&str> = content.lines().collect();
        let mut preamble = String::new();
        let mut tasks: Vec<TaskItem> = Vec::new();
        let mut postamble = String::new();
        let mut in_tasks = false;
        let mut post_tasks = false;

        // Stack for tracking nesting: (indent_level, parent_index_path)
        let mut task_stack: Vec<(usize, Vec<usize>)> = Vec::new();

        let checkbox_re = Regex::new(r"^(\s*)- \[([ x\-])\]\*?\s+(.+)$").unwrap();

        for line in &lines {
            if post_tasks {
                postamble.push_str(line);
                postamble.push('\n');
                continue;
            }

            if let Some(caps) = checkbox_re.captures(line) {
                in_tasks = true;
                let indent_str = caps.get(1).unwrap().as_str();
                let indent = indent_str.len();
                let status_char = caps.get(2).unwrap().as_str();
                let desc_raw = caps.get(3).unwrap().as_str().to_string();

                let status = match status_char {
                    "x" => TaskStatus::Completed,
                    "-" => TaskStatus::InProgress,
                    _ => TaskStatus::NotStarted,
                };

                // Extract task ID (e.g. "1.1" or "2") from start of description
                let (id, description) = Self::extract_id_and_desc(&desc_raw);

                // Extract requirement_ref from description (e.g. "_需求: 1.6, 5.2_" or "(Req 3.1)")
                let requirement_ref = Self::extract_requirement_ref(&desc_raw);

                let item = TaskItem {
                    id,
                    description,
                    status,
                    requirement_ref,
                    children: Vec::new(),
                };

                // Determine nesting level based on indent
                if indent == 0 || task_stack.is_empty() {
                    // Top-level task
                    tasks.push(item);
                    task_stack.clear();
                    task_stack.push((indent, vec![tasks.len() - 1]));
                } else {
                    // Find parent: walk stack backwards to find a task with smaller indent
                    while let Some(&(parent_indent, _)) = task_stack.last() {
                        if parent_indent >= indent {
                            task_stack.pop();
                        } else {
                            break;
                        }
                    }

                    if let Some((_, ref path)) = task_stack.last().cloned() {
                        // Add as child of the task at path
                        let child_idx = Self::add_child_at_path(&mut tasks, path, item);
                        let mut new_path = path.clone();
                        new_path.push(child_idx);
                        task_stack.push((indent, new_path));
                    } else {
                        // No parent found, treat as top-level
                        tasks.push(item);
                        task_stack.clear();
                        task_stack.push((indent, vec![tasks.len() - 1]));
                    }
                }
            } else if in_tasks && line.trim().is_empty() && !lines.iter().skip(lines.iter().position(|l| std::ptr::eq(*l, *line)).unwrap_or(0) + 1).any(|l| checkbox_re.is_match(l)) {
                // No more checkboxes after this blank line — start postamble
                post_tasks = true;
                postamble.push_str(line);
                postamble.push('\n');
            } else if in_tasks {
                // Non-checkbox line within task section — could be description continuation
                // Append to postamble if no more tasks follow, otherwise skip (part of task desc)
                // For simplicity, we treat non-checkbox lines after tasks start as part of the
                // last task's context (ignored in structure, preserved in serialize via description)
            } else {
                preamble.push_str(line);
                preamble.push('\n');
            }
        }

        Ok(TaskDocument { preamble, tasks, postamble })
    }

    /// Serialize a TaskDocument back to Markdown.
    pub fn serialize(doc: &TaskDocument) -> String {
        let mut output = doc.preamble.clone();
        for task in &doc.tasks {
            Self::serialize_task(&mut output, task, 0);
        }
        if !doc.postamble.is_empty() {
            output.push_str(&doc.postamble);
        }
        output
    }

    /// Update the status of a task by ID.
    pub fn update_status(doc: &mut TaskDocument, task_id: &str, status: TaskStatus) -> Result<(), SpecError> {
        if Self::update_status_recursive(&mut doc.tasks, task_id, &status) {
            Ok(())
        } else {
            Err(SpecError::TaskNotFound(task_id.to_string()))
        }
    }

    /// Auto-complete parent tasks when all children are completed.
    pub fn auto_complete_parents(doc: &mut TaskDocument) {
        Self::auto_complete_recursive(&mut doc.tasks);
    }

    /// Get the next pending (NotStarted) task.
    pub fn next_pending(doc: &TaskDocument) -> Option<&TaskItem> {
        Self::find_next_pending(&doc.tasks)
    }

    /// Calculate task progress statistics.
    pub fn progress(doc: &TaskDocument) -> TaskProgress {
        let mut total = 0usize;
        let mut completed = 0usize;
        let mut in_progress = 0usize;
        Self::count_leaves(&doc.tasks, &mut total, &mut completed, &mut in_progress);
        TaskProgress { total, completed, in_progress }
    }

    // ── Private helpers ──

    fn extract_id_and_desc(raw: &str) -> (String, String) {
        let id_re = Regex::new(r"^(\d+(?:\.\d+)*)\s+(.+)$").unwrap();
        if let Some(caps) = id_re.captures(raw) {
            (caps[1].to_string(), caps[2].to_string())
        } else {
            // No numeric ID found, use the full text as description and generate a placeholder ID
            (String::new(), raw.to_string())
        }
    }

    fn extract_requirement_ref(desc: &str) -> Option<String> {
        // Match patterns like "_需求: 1.6, 5.2_" or "_Requirements: 1.2, 3.4_"
        let req_re = Regex::new(r"_(?:需求|Requirements?):\s*([^_]+)_").unwrap();
        req_re.captures(desc).map(|caps| caps[1].trim().to_string())
    }

    fn add_child_at_path(tasks: &mut Vec<TaskItem>, path: &[usize], item: TaskItem) -> usize {
        if path.is_empty() {
            tasks.push(item);
            return tasks.len() - 1;
        }
        let mut current = &mut tasks[path[0]];
        for &idx in &path[1..] {
            current = &mut current.children[idx];
        }
        current.children.push(item);
        current.children.len() - 1
    }

    fn serialize_task(output: &mut String, task: &TaskItem, indent: usize) {
        let indent_str = " ".repeat(indent);
        let status_char = match task.status {
            TaskStatus::NotStarted => " ",
            TaskStatus::InProgress => "-",
            TaskStatus::Completed => "x",
        };
        let id_prefix = if task.id.is_empty() {
            String::new()
        } else {
            format!("{} ", task.id)
        };
        output.push_str(&format!("{}- [{}] {}{}\n", indent_str, status_char, id_prefix, task.description));
        for child in &task.children {
            Self::serialize_task(output, child, indent + 2);
        }
    }

    fn update_status_recursive(tasks: &mut Vec<TaskItem>, task_id: &str, status: &TaskStatus) -> bool {
        for task in tasks.iter_mut() {
            if task.id == task_id {
                task.status = status.clone();
                return true;
            }
            if Self::update_status_recursive(&mut task.children, task_id, status) {
                return true;
            }
        }
        false
    }

    fn auto_complete_recursive(tasks: &mut Vec<TaskItem>) {
        for task in tasks.iter_mut() {
            if !task.children.is_empty() {
                Self::auto_complete_recursive(&mut task.children);
                let all_complete = task.children.iter().all(|c| c.status == TaskStatus::Completed);
                if all_complete {
                    task.status = TaskStatus::Completed;
                }
            }
        }
    }

    fn find_next_pending(tasks: &[TaskItem]) -> Option<&TaskItem> {
        for task in tasks {
            if task.children.is_empty() {
                if task.status == TaskStatus::NotStarted {
                    return Some(task);
                }
            } else {
                if let Some(found) = Self::find_next_pending(&task.children) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn count_leaves(tasks: &[TaskItem], total: &mut usize, completed: &mut usize, in_progress: &mut usize) {
        for task in tasks {
            if task.children.is_empty() {
                *total += 1;
                match task.status {
                    TaskStatus::Completed => *completed += 1,
                    TaskStatus::InProgress => *in_progress += 1,
                    TaskStatus::NotStarted => {}
                }
            } else {
                Self::count_leaves(&task.children, total, completed, in_progress);
            }
        }
    }
}

// ─── SpecStore ─────────────────────────────────────────────────────────────────

/// File storage layer for Spec documents.
pub struct SpecStore {
    base_dir: PathBuf,
}

impl SpecStore {
    pub fn new(cwd: &Path) -> Self {
        Self {
            base_dir: cwd.join(".baoclaw").join("specs"),
        }
    }

    /// Ensure the specs base directory exists.
    pub fn ensure_base_dir(&self) -> Result<(), SpecError> {
        std::fs::create_dir_all(&self.base_dir)?;
        Ok(())
    }

    /// Create a new spec directory.
    pub fn create_spec_dir(&self, feature_name: &str) -> Result<PathBuf, SpecError> {
        let dir = self.base_dir.join(feature_name);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Check if a spec exists.
    pub fn spec_exists(&self, feature_name: &str) -> bool {
        self.base_dir.join(feature_name).join(".config.json").exists()
    }

    /// Read the spec config.
    pub fn read_config(&self, feature_name: &str) -> Result<SpecConfig, SpecError> {
        let path = self.base_dir.join(feature_name).join(".config.json");
        if !path.exists() {
            return Err(SpecError::NotFound(feature_name.to_string()));
        }
        let content = std::fs::read_to_string(&path)?;
        let config: SpecConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Write the spec config.
    pub fn write_config(&self, feature_name: &str, config: &SpecConfig) -> Result<(), SpecError> {
        let dir = self.base_dir.join(feature_name);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(".config.json");
        let json = serde_json::to_string_pretty(config)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Read a document file (requirements.md, design.md, tasks.md).
    pub fn read_doc(&self, feature_name: &str, filename: &str) -> Result<String, SpecError> {
        let path = self.base_dir.join(feature_name).join(filename);
        if !path.exists() {
            return Err(SpecError::NotFound(format!("{}/{}", feature_name, filename)));
        }
        Ok(std::fs::read_to_string(&path)?)
    }

    /// Write a document file.
    pub fn write_doc(&self, feature_name: &str, filename: &str, content: &str) -> Result<(), SpecError> {
        let dir = self.base_dir.join(feature_name);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(filename);
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// List all spec directory names.
    pub fn list_spec_names(&self) -> Result<Vec<String>, SpecError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    // Only include dirs that have a .config.json
                    if entry.path().join(".config.json").exists() {
                        names.push(name.to_string());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }
}

// ─── SpecEngine ────────────────────────────────────────────────────────────────

/// Core engine for Spec-Driven Development.
pub struct SpecEngine {
    store: SpecStore,
}

impl SpecEngine {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            store: SpecStore::new(&cwd),
        }
    }

    /// Create a new spec.
    pub fn create_spec(
        &self,
        feature_name: &str,
        workflow: SpecWorkflow,
        spec_type: SpecType,
    ) -> Result<SpecConfig, SpecError> {
        if self.store.spec_exists(feature_name) {
            return Err(SpecError::AlreadyExists(feature_name.to_string()));
        }
        self.store.ensure_base_dir()?;
        self.store.create_spec_dir(feature_name)?;

        let now = chrono::Utc::now().to_rfc3339();
        let initial_phase = match workflow {
            SpecWorkflow::RequirementsFirst => SpecPhase::Requirements,
            SpecWorkflow::DesignFirst => SpecPhase::Design,
        };
        let config = SpecConfig {
            workflow,
            phase: initial_phase,
            spec_type,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.write_config(feature_name, &config)?;
        Ok(config)
    }

    /// List all specs in the project.
    pub fn list_specs(&self) -> Result<Vec<SpecSummary>, SpecError> {
        let names = self.store.list_spec_names()?;
        let mut summaries = Vec::new();
        for name in names {
            if let Ok(config) = self.store.read_config(&name) {
                let task_progress = self.get_status(&name).ok();
                summaries.push(SpecSummary {
                    feature_name: name,
                    workflow: config.workflow,
                    phase: config.phase,
                    spec_type: config.spec_type,
                    task_progress,
                });
            }
        }
        Ok(summaries)
    }

    /// Get spec details.
    pub fn get_spec(&self, feature_name: &str) -> Result<SpecSummary, SpecError> {
        let config = self.store.read_config(feature_name)?;
        let task_progress = self.get_status(feature_name).ok();
        Ok(SpecSummary {
            feature_name: feature_name.to_string(),
            workflow: config.workflow,
            phase: config.phase,
            spec_type: config.spec_type,
            task_progress,
        })
    }

    /// Get task progress for a spec.
    pub fn get_status(&self, feature_name: &str) -> Result<TaskProgress, SpecError> {
        match self.store.read_doc(feature_name, "tasks.md") {
            Ok(content) => {
                let doc = TaskTracker::parse(&content)?;
                Ok(TaskTracker::progress(&doc))
            }
            Err(_) => Ok(TaskProgress { total: 0, completed: 0, in_progress: 0 }),
        }
    }

    /// Read a phase document.
    pub fn read_phase_doc(&self, feature_name: &str, phase: &str) -> Result<String, SpecError> {
        let filename = match phase {
            "requirements" => "requirements.md",
            "design" => "design.md",
            "tasks" => "tasks.md",
            other => return Err(SpecError::PhaseNotAvailable {
                phase: other.to_string(),
                current: self.store.read_config(feature_name)?.phase,
            }),
        };
        self.store.read_doc(feature_name, filename)
    }

    /// Write a phase document.
    pub fn write_phase_doc(&self, feature_name: &str, phase: &str, content: &str) -> Result<(), SpecError> {
        let filename = match phase {
            "requirements" => "requirements.md",
            "design" => "design.md",
            "tasks" => "tasks.md",
            other => return Err(SpecError::PhaseNotAvailable {
                phase: other.to_string(),
                current: self.store.read_config(feature_name)?.phase,
            }),
        };
        self.store.write_doc(feature_name, filename, content)?;
        // Update timestamp
        let mut config = self.store.read_config(feature_name)?;
        config.updated_at = chrono::Utc::now().to_rfc3339();
        self.store.write_config(feature_name, &config)?;
        Ok(())
    }

    /// Advance the spec to a new phase (validates transition).
    pub fn advance_phase(&self, feature_name: &str, new_phase: SpecPhase) -> Result<(), SpecError> {
        let mut config = self.store.read_config(feature_name)?;
        if !Self::is_valid_transition(&config.phase, &new_phase) {
            return Err(SpecError::InvalidPhaseTransition {
                current: config.phase,
                target: new_phase,
            });
        }
        config.phase = new_phase;
        config.updated_at = chrono::Utc::now().to_rfc3339();
        self.store.write_config(feature_name, &config)?;
        Ok(())
    }

    /// Get the next pending task.
    pub fn next_task(&self, feature_name: &str) -> Result<Option<TaskItem>, SpecError> {
        let content = self.store.read_doc(feature_name, "tasks.md")?;
        let doc = TaskTracker::parse(&content)?;
        Ok(TaskTracker::next_pending(&doc).cloned())
    }

    /// Update a task's status.
    pub fn update_task_status(
        &self,
        feature_name: &str,
        task_id: &str,
        status: TaskStatus,
    ) -> Result<(), SpecError> {
        let content = self.store.read_doc(feature_name, "tasks.md")?;
        let mut doc = TaskTracker::parse(&content)?;
        TaskTracker::update_status(&mut doc, task_id, status)?;
        TaskTracker::auto_complete_parents(&mut doc);
        let serialized = TaskTracker::serialize(&doc);
        self.store.write_doc(feature_name, "tasks.md", &serialized)?;
        Ok(())
    }

    /// Build context for task execution (injects relevant requirements + design snippets).
    pub fn build_task_context(&self, feature_name: &str, task_id: &str) -> Result<String, SpecError> {
        let tasks_content = self.store.read_doc(feature_name, "tasks.md")?;
        let doc = TaskTracker::parse(&tasks_content)?;

        // Find the task
        let task = Self::find_task_by_id(&doc.tasks, task_id)
            .ok_or_else(|| SpecError::TaskNotFound(task_id.to_string()))?;

        let mut context = String::new();
        context.push_str(&format!("# Current Task: {} {}\n\n", task.id, task.description));

        // Inject requirement references if available
        if let Some(ref req_ref) = task.requirement_ref {
            if let Ok(requirements) = self.store.read_doc(feature_name, "requirements.md") {
                context.push_str("## Relevant Requirements\n\n");
                // Extract sections matching the requirement references
                for req_id in req_ref.split(',').map(|s| s.trim()) {
                    let pattern = format!("### 需求 {}:", req_id.split('.').next().unwrap_or(req_id));
                    if let Some(start) = requirements.find(&pattern) {
                        let section = &requirements[start..];
                        let end = section[1..].find("\n### ").map(|i| i + 1).unwrap_or(section.len().min(2000));
                        context.push_str(&section[..end]);
                        context.push_str("\n\n");
                    }
                }
            }
        }

        // Inject design context
        if let Ok(design) = self.store.read_doc(feature_name, "design.md") {
            context.push_str("## Relevant Design\n\n");
            // Include first 3000 chars of design as context (truncated for context window)
            let truncated: String = design.chars().take(3000).collect();
            context.push_str(&truncated);
            if design.len() > 3000 {
                context.push_str("\n...[truncated]");
            }
            context.push('\n');
        }

        Ok(context)
    }

    // ── Private helpers ──

    fn is_valid_transition(current: &SpecPhase, target: &SpecPhase) -> bool {
        matches!(
            (current, target),
            (SpecPhase::Requirements, SpecPhase::RequirementsComplete)
            | (SpecPhase::RequirementsComplete, SpecPhase::Design)
            | (SpecPhase::Design, SpecPhase::DesignComplete)
            | (SpecPhase::DesignComplete, SpecPhase::Tasks)
            | (SpecPhase::Tasks, SpecPhase::TasksComplete)
        )
    }

    fn find_task_by_id<'a>(tasks: &'a [TaskItem], task_id: &str) -> Option<&'a TaskItem> {
        for task in tasks {
            if task.id == task_id {
                return Some(task);
            }
            if let Some(found) = Self::find_task_by_id(&task.children, task_id) {
                return Some(found);
            }
        }
        None
    }
}
