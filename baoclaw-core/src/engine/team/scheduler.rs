//! DAG Scheduler for Sub-Agent Teams.
//!
//! This module provides topological sorting and dependency resolution for DAG-based
//! team execution. It supports parallel execution of independent nodes while
//! respecting dependency ordering.
//!
//! # Architecture
//!
//! - `DagScheduler` - Main scheduler that manages DAG execution
//! - `DagNode` - A node in the DAG with dependencies and execution state
//! - `ExecutionWave` - A set of nodes that can execute in parallel
//!
//! # Example
//!
//! ```rust,ignore
//! use baoclaw_core::engine::team::scheduler::{DagScheduler, DagNode};
//!
//! // Create a DAG
//! let mut scheduler = DagScheduler::new();
//!
//! // Add nodes with dependencies
//! scheduler.add_node(DagNode::new("a", "Task A"));
//! scheduler.add_node(DagNode::new("b", "Task B").with_dependency("a"));
//! scheduler.add_node(DagNode::new("c", "Task C").with_dependency("a"));
//! scheduler.add_node(DagNode::new("d", "Task D").with_dependencies(&["b", "c"]));
//!
//! // Get execution order (topological sort)
//! let order = scheduler.topological_sort()?;
//!
//! // Get execution waves (nodes that can run in parallel)
//! let waves = scheduler.execution_waves()?;
//!
//! // Execute ready nodes in parallel with max parallelism limit
//! let ready = scheduler.get_ready_nodes(&completed, 3);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use super::types::{AgentTeam, SubAgent, SubAgentStatus};

/// Error type for DAG scheduler operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchedulerError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref node_id) = self.node_id {
            write!(f, "[{}] {}: {}", self.code, node_id, self.message)
        } else {
            write!(f, "[{}]: {}", self.code, self.message)
        }
    }
}

impl std::error::Error for SchedulerError {}

/// Status of a DAG node.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum NodeStatus {
    /// Node is waiting for dependencies.
    #[default]
    Pending,
    /// Node is ready to execute (all dependencies satisfied).
    Ready,
    /// Node is currently executing.
    Running,
    /// Node completed successfully.
    Completed,
    /// Node execution failed.
    Failed,
    /// Node was skipped (dependency failed).
    Skipped,
}


impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Ready => write!(f, "ready"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

impl From<SubAgentStatus> for NodeStatus {
    fn from(status: SubAgentStatus) -> Self {
        match status {
            SubAgentStatus::Pending => NodeStatus::Pending,
            SubAgentStatus::Running => NodeStatus::Running,
            SubAgentStatus::Completed => NodeStatus::Completed,
            SubAgentStatus::Failed => NodeStatus::Failed,
            SubAgentStatus::Skipped => NodeStatus::Skipped,
        }
    }
}

impl From<NodeStatus> for SubAgentStatus {
    fn from(status: NodeStatus) -> Self {
        match status {
            NodeStatus::Pending => SubAgentStatus::Pending,
            NodeStatus::Ready => SubAgentStatus::Pending,
            NodeStatus::Running => SubAgentStatus::Running,
            NodeStatus::Completed => SubAgentStatus::Completed,
            NodeStatus::Failed => SubAgentStatus::Failed,
            NodeStatus::Skipped => SubAgentStatus::Skipped,
        }
    }
}

/// A node in the DAG.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DagNode {
    /// Unique identifier for this node.
    pub id: String,
    
    /// Human-readable name or description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    
    /// The task prompt for this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    
    /// Dependencies (IDs of nodes this node depends on).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    
    /// Current status of this node.
    #[serde(default)]
    pub status: NodeStatus,
    
    /// Priority (higher values execute first when multiple nodes are ready).
    #[serde(default)]
    pub priority: i32,
    
    /// Maximum parallel instances allowed for this node type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,
    
    /// Additional metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl DagNode {
    /// Create a new DAG node with the given ID.
    pub fn new(id: impl Into<String>, prompt: impl Into<String>) -> Self {
        let id = id.into();
        let prompt = prompt.into();
        Self {
            id: id.clone(),
            name: Some(id),
            prompt: Some(prompt),
            dependencies: Vec::new(),
            status: NodeStatus::default(),
            priority: 0,
            max_parallel: None,
            metadata: HashMap::new(),
        }
    }
    
    /// Set the name for this node.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
    
    /// Add a single dependency.
    pub fn with_dependency(mut self, dep_id: impl Into<String>) -> Self {
        self.dependencies.push(dep_id.into());
        self
    }
    
    /// Add multiple dependencies.
    pub fn with_dependencies(mut self, dep_ids: &[&str]) -> Self {
        self.dependencies.extend(dep_ids.iter().map(|s| s.to_string()));
        self
    }
    
    /// Set the priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
    
    /// Set max parallel instances.
    pub fn with_max_parallel(mut self, max: usize) -> Self {
        self.max_parallel = Some(max);
        self
    }
    
    /// Check if this node is ready (all dependencies are in the completed set).
    pub fn is_ready(&self, completed: &HashSet<&str>) -> bool {
        self.status == NodeStatus::Pending
            && self.dependencies.iter().all(|dep| completed.contains(dep.as_str()))
    }
    
    /// Check if this node can be scheduled (pending or ready status).
    pub fn can_schedule(&self) -> bool {
        matches!(self.status, NodeStatus::Pending | NodeStatus::Ready)
    }
}

impl From<SubAgent> for DagNode {
    fn from(agent: SubAgent) -> Self {
        Self {
            id: agent.id,
            name: Some(agent.name),
            prompt: Some(agent.prompt),
            dependencies: agent.dependencies,
            status: NodeStatus::from(agent.status),
            priority: agent.metadata.get("priority")
                .and_then(|p| p.parse().ok())
                .unwrap_or(0),
            max_parallel: None,
            metadata: agent.metadata,
        }
    }
}

/// A wave of nodes that can execute in parallel.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionWave {
    /// Wave number (0 = first wave).
    pub wave: usize,
    
    /// Nodes in this wave.
    pub nodes: Vec<String>,
    
    /// Whether this wave can be executed in parallel.
    pub parallel: bool,
}

impl ExecutionWave {
    /// Create a new execution wave.
    pub fn new(wave: usize, nodes: Vec<String>, parallel: bool) -> Self {
        Self { wave, nodes, parallel }
    }
}

/// DAG scheduler for managing dependency-based execution.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DagScheduler {
    /// All nodes in the DAG.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    nodes: HashMap<String, DagNode>,
    
    /// Reverse dependency map: node_id -> nodes that depend on it.
    #[serde(skip)]
    dependents: HashMap<String, Vec<String>>,
    
    /// In-degree count for each node (number of pending dependencies).
    #[serde(skip)]
    in_degree: HashMap<String, usize>,
    
    /// Maximum parallelism (max nodes that can run concurrently).
    #[serde(default = "default_max_parallelism")]
    max_parallelism: usize,
}

fn default_max_parallelism() -> usize {
    10
}

impl DagScheduler {
    /// Create a new empty DAG scheduler.
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Create a new scheduler with max parallelism.
    pub fn with_max_parallelism(max: usize) -> Self {
        Self {
            max_parallelism: max,
            ..Self::default()
        }
    }
    
    /// Set max parallelism.
    pub fn set_max_parallelism(&mut self, max: usize) {
        self.max_parallelism = max;
    }
    
    /// Get max parallelism.
    pub fn max_parallelism(&self) -> usize {
        self.max_parallelism
    }
    
    /// Build a scheduler from an AgentTeam.
    pub fn from_team(team: &AgentTeam) -> Result<Self, SchedulerError> {
        let mut scheduler = Self::new();
        
        for agent in &team.agents {
            scheduler.add_node(DagNode::from(agent.clone()))?;
        }
        
        scheduler.build()?;
        Ok(scheduler)
    }
    
    /// Add a node to the DAG.
    pub fn add_node(&mut self, node: DagNode) -> Result<(), SchedulerError> {
        let id = node.id.clone();
        
        // Check for duplicate
        if self.nodes.contains_key(&id) {
            return Err(SchedulerError {
                code: "duplicate_node".to_string(),
                message: format!("Node '{}' already exists", id),
                node_id: Some(id),
            });
        }
        
        self.nodes.insert(id, node);
        Ok(())
    }
    
    /// Remove a node from the DAG.
    pub fn remove_node(&mut self, id: &str) -> Option<DagNode> {
        let node = self.nodes.remove(id)?;
        
        // Remove from dependents
        for dep in &node.dependencies {
            if let Some(deps) = self.dependents.get_mut(dep) {
                deps.retain(|d| d != id);
            }
        }
        
        // Remove node's dependents entry
        self.dependents.remove(id);
        
        Some(node)
    }
    
    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&DagNode> {
        self.nodes.get(id)
    }
    
    /// Get a mutable node by ID.
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut DagNode> {
        self.nodes.get_mut(id)
    }
    
    /// Get all node IDs.
    pub fn node_ids(&self) -> Vec<&str> {
        self.nodes.keys().map(|s| s.as_str()).collect()
    }
    
    /// Get node count.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    
    /// Build internal structures for scheduling.
    pub fn build(&mut self) -> Result<(), SchedulerError> {
        // Clear previous state
        self.dependents.clear();
        self.in_degree.clear();
        
        // Build reverse dependency map and in-degree counts
        for (id, node) in &self.nodes {
            // Validate dependencies exist
            for dep in &node.dependencies {
                if !self.nodes.contains_key(dep) {
                    return Err(SchedulerError {
                        code: "missing_dependency".to_string(),
                        message: format!("Dependency '{}' does not exist", dep),
                        node_id: Some(id.clone()),
                    });
                }
            }
            
            self.in_degree.insert(id.clone(), node.dependencies.len());
            
            for dep in &node.dependencies {
                self.dependents
                    .entry(dep.clone())
                    .or_default()
                    .push(id.clone());
            }
        }
        
        // Check for cycles
        self.detect_cycles()?;
        
        Ok(())
    }
    
    /// Detect cycles in the DAG.
    fn detect_cycles(&self) -> Result<(), SchedulerError> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        
        for node_id in self.nodes.keys() {
            if self.has_cycle(node_id, &mut visited, &mut rec_stack)? {
                return Err(SchedulerError {
                    code: "cycle_detected".to_string(),
                    message: format!("DAG contains a cycle involving node '{}'", node_id),
                    node_id: Some(node_id.clone()),
                });
            }
        }
        
        Ok(())
    }
    
    /// Recursive DFS helper for cycle detection.
    fn has_cycle(
        &self,
        node_id: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> Result<bool, SchedulerError> {
        if rec_stack.contains(node_id) {
            return Ok(true);
        }
        
        if visited.contains(node_id) {
            return Ok(false);
        }
        
        visited.insert(node_id.to_string());
        rec_stack.insert(node_id.to_string());
        
        if let Some(dependents) = self.dependents.get(node_id) {
            for dep in dependents {
                if self.has_cycle(dep, visited, rec_stack)? {
                    return Ok(true);
                }
            }
        }
        
        rec_stack.remove(node_id);
        Ok(false)
    }
    
    /// Perform topological sort.
    ///
    /// Returns nodes in an order where dependencies are satisfied.
    /// Uses Kahn's algorithm for stable, deterministic ordering.
    pub fn topological_sort(&self) -> Result<Vec<String>, SchedulerError> {
        if self.nodes.is_empty() {
            return Ok(Vec::new());
        }
        
        let mut in_degree = self.in_degree.clone();
        
        let queue: VecDeque<String> = self
            .nodes
            .keys()
            .filter(|id| in_degree.get(*id) == Some(&0))
            .cloned()
            .collect();
        
        let mut queue: Vec<String> = queue.into_iter().collect();
        queue.sort_by(|a, b| {
            let pa = self.nodes.get(a).map(|n| n.priority).unwrap_or(0);
            let pb = self.nodes.get(b).map(|n| n.priority).unwrap_or(0);
            pb.cmp(&pa).then_with(|| a.cmp(b))
        });
        
        let mut result = Vec::new();
        
        while !queue.is_empty() {
            let node_id = queue.remove(0);
            result.push(node_id.clone());
            
            if let Some(dependents) = self.dependents.get(&node_id) {
                for dep in dependents {
                    if let Some(deg) = in_degree.get_mut(dep) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(dep.clone());
                        }
                    }
                }
            }
            
            queue.sort_by(|a, b| {
                let pa = self.nodes.get(a).map(|n| n.priority).unwrap_or(0);
                let pb = self.nodes.get(b).map(|n| n.priority).unwrap_or(0);
                pb.cmp(&pa).then_with(|| a.cmp(b))
            });
        }
        
        if result.len() != self.nodes.len() {
            return Err(SchedulerError {
                code: "cycle_detected".to_string(),
                message: "DAG contains a cycle, cannot perform topological sort".to_string(),
                node_id: None,
            });
        }
        
        Ok(result)
    }
    
    /// Get execution waves (groups of nodes that can run in parallel).
    pub fn execution_waves(&self) -> Result<Vec<ExecutionWave>, SchedulerError> {
        if self.nodes.is_empty() {
            return Ok(Vec::new());
        }
        
        let mut in_degree = self.in_degree.clone();
        let mut waves = Vec::new();
        let mut completed: HashSet<String> = HashSet::new();
        
        loop {
            let mut ready: Vec<String> = self
                .nodes
                .keys()
                .filter(|id| {
                    !completed.contains(*id) && in_degree.get(*id) == Some(&0)
                })
                .cloned()
                .collect();
            
            if ready.is_empty() {
                break;
            }
            
            ready.sort_by(|a, b| {
                let pa = self.nodes.get(a).map(|n| n.priority).unwrap_or(0);
                let pb = self.nodes.get(b).map(|n| n.priority).unwrap_or(0);
                pb.cmp(&pa).then_with(|| a.cmp(b))
            });
            
            for id in &ready {
                completed.insert(id.clone());
                
                if let Some(dependents) = self.dependents.get(id) {
                    for dep in dependents {
                        if let Some(deg) = in_degree.get_mut(dep) {
                            *deg -= 1;
                        }
                    }
                }
            }
            
            let wave_num = waves.len();
            let parallel = ready.len() > 1;
            waves.push(ExecutionWave::new(wave_num, ready, parallel));
        }
        
        let processed_count: usize = waves.iter().map(|w| w.nodes.len()).sum();
        if processed_count != self.nodes.len() {
            return Err(SchedulerError {
                code: "cycle_detected".to_string(),
                message: "DAG contains a cycle, cannot compute execution waves".to_string(),
                node_id: None,
            });
        }
        
        Ok(waves)
    }
    
    /// Get nodes that are ready to execute (all dependencies completed).
    pub fn ready_nodes(&self, completed: &HashSet<&str>) -> Vec<&DagNode> {
        self.nodes
            .values()
            .filter(|node| node.is_ready(completed))
            .collect()
    }
    
    /// Get ready nodes respecting max parallelism.
    pub fn get_ready_nodes(&self, completed: &HashSet<&str>, currently_running: usize) -> Vec<String> {
        if currently_running >= self.max_parallelism {
            return Vec::new();
        }
        
        let remaining_slots = self.max_parallelism - currently_running;
        
        let mut ready: Vec<String> = self
            .nodes
            .values()
            .filter(|node| node.is_ready(completed))
            .map(|node| node.id.clone())
            .collect();
        
        ready.sort_by(|a, b| {
            let pa = self.nodes.get(a).map(|n| n.priority).unwrap_or(0);
            let pb = self.nodes.get(b).map(|n| n.priority).unwrap_or(0);
            pb.cmp(&pa).then_with(|| a.cmp(b))
        });
        
        ready.truncate(remaining_slots);
        ready
    }
    
    /// Get the IDs of all nodes that depend on a given node.
    pub fn get_dependents(&self, node_id: &str) -> Vec<&str> {
        self.dependents
            .get(node_id)
            .map(|deps| deps.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }
    
    /// Get the dependencies of a given node.
    pub fn get_dependencies(&self, node_id: &str) -> Vec<&str> {
        self.nodes
            .get(node_id)
            .map(|node| node.dependencies.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }
    
    /// Mark a node as running.
    pub fn start_node(&mut self, id: &str) -> Result<(), SchedulerError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| SchedulerError {
                code: "node_not_found".to_string(),
                message: format!("Node '{}' not found", id),
                node_id: Some(id.to_string()),
            })?;
        
        node.status = NodeStatus::Running;
        Ok(())
    }
    
    /// Mark a node as completed.
    pub fn complete_node(&mut self, id: &str) -> Result<(), SchedulerError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| SchedulerError {
                code: "node_not_found".to_string(),
                message: format!("Node '{}' not found", id),
                node_id: Some(id.to_string()),
            })?;
        
        node.status = NodeStatus::Completed;
        Ok(())
    }
    
    /// Mark a node as failed.
    pub fn fail_node(&mut self, id: &str, error: Option<&str>) -> Result<(), SchedulerError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| SchedulerError {
                code: "node_not_found".to_string(),
                message: format!("Node '{}' not found", id),
                node_id: Some(id.to_string()),
            })?;
        
        node.status = NodeStatus::Failed;
        if let Some(err) = error {
            node.metadata.insert("error".to_string(), err.to_string());
        }
        Ok(())
    }
    
    /// Mark all dependents of a failed node as skipped.
    pub fn skip_dependents(&mut self, failed_id: &str, reason: &str) -> Vec<String> {
        let mut skipped = Vec::new();
        let mut to_skip: VecDeque<String> = VecDeque::new();
        
        if let Some(dependents) = self.dependents.get(failed_id).cloned() {
            for dep in dependents {
                to_skip.push_back(dep);
            }
        }
        
        while let Some(node_id) = to_skip.pop_front() {
            if let Some(node) = self.nodes.get_mut(&node_id) {
                if node.status == NodeStatus::Pending || node.status == NodeStatus::Ready {
                    node.status = NodeStatus::Skipped;
                    node.metadata.insert("skip_reason".to_string(), reason.to_string());
                    skipped.push(node_id.clone());
                    
                    if let Some(deps) = self.dependents.get(&node_id).cloned() {
                        for dep in deps {
                            if !skipped.contains(&dep) {
                                to_skip.push_back(dep);
                            }
                        }
                    }
                }
            }
        }
        
        skipped
    }
    
    /// Get the critical path (longest path through the DAG).
    pub fn critical_path(&self) -> Result<Vec<String>, SchedulerError> {
        let mut longest_path: HashMap<String, usize> = HashMap::new();
        let mut predecessor: HashMap<String, String> = HashMap::new();
        
        let topo_order = self.topological_sort()?;
        
        for node_id in &topo_order {
            let node = self.nodes.get(node_id).unwrap();
            let mut max_pred_len = 0;
            let mut max_pred: Option<String> = None;
            
            for dep in &node.dependencies {
                if let Some(&len) = longest_path.get(dep) {
                    if len > max_pred_len {
                        max_pred_len = len;
                        max_pred = Some(dep.clone());
                    }
                }
            }
            
            longest_path.insert(node_id.clone(), max_pred_len + 1);
            if let Some(pred) = max_pred {
                predecessor.insert(node_id.clone(), pred);
            }
        }
        
        let (end_node, _) = longest_path
            .into_iter()
            .max_by_key(|(_, len)| *len)
            .unwrap_or((topo_order.last().cloned().unwrap_or_default(), 0));
        
        let mut path = vec![end_node.clone()];
        let mut current = end_node;
        
        while let Some(pred) = predecessor.get(&current) {
            path.push(pred.clone());
            current = pred.clone();
        }
        
        path.reverse();
        Ok(path)
    }
    
    /// Get statistics about the DAG.
    #[allow(clippy::field_reassign_with_default)]
    pub fn stats(&self) -> DagStats {
        let mut stats = DagStats::default();
        stats.total_nodes = self.nodes.len();
        stats.max_parallelism = self.max_parallelism;
        
        for node in self.nodes.values() {
            stats.total_dependencies += node.dependencies.len();
            match node.status {
                NodeStatus::Pending => stats.pending_count += 1,
                NodeStatus::Ready => stats.ready_count += 1,
                NodeStatus::Running => stats.running_count += 1,
                NodeStatus::Completed => stats.completed_count += 1,
                NodeStatus::Failed => stats.failed_count += 1,
                NodeStatus::Skipped => stats.skipped_count += 1,
            }
        }
        
        if let Ok(waves) = self.execution_waves() {
            stats.max_depth = waves.len().saturating_sub(1);
            stats.total_waves = waves.len();
            stats.parallelism_ratio = if waves.is_empty() {
                0.0
            } else {
                stats.total_nodes as f64 / waves.len() as f64
            };
            stats.max_wave_size = waves.iter().map(|w| w.nodes.len()).max().unwrap_or(0);
        }
        
        stats
    }
    
    /// Check if all nodes are done (completed, failed, or skipped).
    pub fn is_complete(&self) -> bool {
        self.nodes.values().all(|node| {
            matches!(
                node.status,
                NodeStatus::Completed | NodeStatus::Failed | NodeStatus::Skipped
            )
        })
    }
    
    /// Check if any node has failed.
    pub fn has_failures(&self) -> bool {
        self.nodes.values().any(|node| node.status == NodeStatus::Failed)
    }
    
    /// Check if execution was successful (all nodes completed, no failures).
    pub fn is_successful(&self) -> bool {
        self.nodes.values().all(|node| node.status == NodeStatus::Completed)
    }
    
    /// Get a summary of the scheduler state.
    pub fn summary(&self) -> String {
        let stats = self.stats();
        format!(
            "DagScheduler: {} nodes ({} pending, {} ready, {} running, {} completed, {} failed, {} skipped), max_depth={}, max_parallelism={}",
            stats.total_nodes,
            stats.pending_count,
            stats.ready_count,
            stats.running_count,
            stats.completed_count,
            stats.failed_count,
            stats.skipped_count,
            stats.max_depth,
            stats.max_parallelism
        )
    }
    
    /// Validate the DAG structure.
    pub fn validate(&self) -> Result<(), Vec<SchedulerError>> {
        let mut errors = Vec::new();
        
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                if !self.nodes.contains_key(dep) {
                    errors.push(SchedulerError {
                        code: "missing_dependency".to_string(),
                        message: format!("Dependency '{}' does not exist", dep),
                        node_id: Some(node.id.clone()),
                    });
                }
            }
        }
        
        let mut clone = self.clone();
        if let Err(e) = clone.build() {
            errors.push(e);
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    
    /// Export the DAG as a DOT graph for visualization.
    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph DAG {\n");
        dot.push_str("  rankdir=TB;\n");
        dot.push_str("  node [shape=box];\n\n");
        
        for node in self.nodes.values() {
            let status = node.status.to_string();
            let color = match node.status {
                NodeStatus::Pending => "gray",
                NodeStatus::Ready => "lightblue",
                NodeStatus::Running => "yellow",
                NodeStatus::Completed => "green",
                NodeStatus::Failed => "red",
                NodeStatus::Skipped => "orange",
            };
            dot.push_str(&format!(
                "  \"{}\" [label=\"{}\\n({})\" style=filled fillcolor={}];\n",
                node.id, node.id, status, color
            ));
        }
        
        dot.push('\n');
        
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                dot.push_str(&format!("  \"{}\" -> \"{}\";\n", dep, node.id));
            }
        }
        
        dot.push_str("}\n");
        dot
    }
}


/// Statistics about a DAG.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DagStats {
    /// Total number of nodes.
    pub total_nodes: usize,
    
    /// Total number of dependency edges.
    pub total_dependencies: usize,
    
    /// Maximum depth of the DAG (number of waves - 1).
    pub max_depth: usize,
    
    /// Total number of waves.
    pub total_waves: usize,
    
    /// Maximum number of nodes in a single wave.
    pub max_wave_size: usize,
    
    /// Maximum parallelism setting.
    pub max_parallelism: usize,
    
    /// Average parallelism (nodes / waves).
    pub parallelism_ratio: f64,
    
    /// Number of pending nodes.
    pub pending_count: usize,
    
    /// Number of ready nodes.
    pub ready_count: usize,
    
    /// Number of running nodes.
    pub running_count: usize,
    
    /// Number of completed nodes.
    pub completed_count: usize,
    
    /// Number of failed nodes.
    pub failed_count: usize,
    
    /// Number of skipped nodes.
    pub skipped_count: usize,
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dag_node_creation() {
        let node = DagNode::new("node-1", "Do something");
        assert_eq!(node.id, "node-1");
        assert_eq!(node.prompt, Some("Do something".to_string()));
        assert_eq!(node.status, NodeStatus::Pending);
        assert!(node.dependencies.is_empty());
    }
    
    #[test]
    fn test_dag_node_with_dependencies() {
        let node = DagNode::new("node-2", "Dependent task")
            .with_dependency("node-1")
            .with_priority(10);
        
        assert_eq!(node.dependencies, vec!["node-1"]);
        assert_eq!(node.priority, 10);
    }
    
    #[test]
    fn test_dag_node_is_ready() {
        let node = DagNode::new("node-2", "Task")
            .with_dependency("node-1");
        
        let mut completed: HashSet<&str> = HashSet::new();
        assert!(!node.is_ready(&completed));
        
        completed.insert("node-1");
        assert!(node.is_ready(&completed));
    }
    
    #[test]
    fn test_scheduler_add_node() {
        let mut scheduler = DagScheduler::new();
        
        let node = DagNode::new("node-1", "Task");
        assert!(scheduler.add_node(node).is_ok());
        assert_eq!(scheduler.node_count(), 1);
    }
    
    #[test]
    fn test_scheduler_duplicate_node() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("node-1", "Task 1")).unwrap();
        let result = scheduler.add_node(DagNode::new("node-1", "Task 2"));
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "duplicate_node");
    }
    
    #[test]
    fn test_scheduler_missing_dependency() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("node-2", "Task 2").with_dependency("node-1")).unwrap();
        let result = scheduler.build();
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "missing_dependency");
    }

    
    #[test]
    fn test_topological_sort_simple() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C")).unwrap();
        
        scheduler.build().unwrap();
        
        let order = scheduler.topological_sort().unwrap();
        assert_eq!(order.len(), 3);
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
        assert!(order.contains(&"c".to_string()));
    }
    
    #[test]
    fn test_topological_sort_with_dependencies() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("d", "Task D").with_dependencies(&["b", "c"])).unwrap();
        
        scheduler.build().unwrap();
        
        let order = scheduler.topological_sort().unwrap();
        
        assert!(order.iter().position(|x| x == "a") < order.iter().position(|x| x == "b"));
        assert!(order.iter().position(|x| x == "a") < order.iter().position(|x| x == "c"));
        assert!(order.iter().position(|x| x == "b") < order.iter().position(|x| x == "d"));
        assert!(order.iter().position(|x| x == "c") < order.iter().position(|x| x == "d"));
    }
    
    #[test]
    fn test_execution_waves() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("d", "Task D").with_dependencies(&["b", "c"])).unwrap();
        
        scheduler.build().unwrap();
        
        let waves = scheduler.execution_waves().unwrap();
        
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].nodes, vec!["a"]);
        assert!(!waves[0].parallel);
        
        assert_eq!(waves[1].nodes.len(), 2);
        assert!(waves[1].nodes.contains(&"b".to_string()));
        assert!(waves[1].nodes.contains(&"c".to_string()));
        assert!(waves[1].parallel);
        
        assert_eq!(waves[2].nodes, vec!["d"]);
    }

    #[test]
    fn test_cycle_detection() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C")).unwrap();
        
        // Create a cycle: a -> b -> c -> a
        scheduler.get_node_mut("a").unwrap().dependencies.push("c".to_string());
        scheduler.get_node_mut("b").unwrap().dependencies.push("a".to_string());
        scheduler.get_node_mut("c").unwrap().dependencies.push("b".to_string());
        
        let result = scheduler.build();
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "cycle_detected");
    }
    
    #[test]
    fn test_skip_dependents() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("d", "Task D").with_dependencies(&["b", "c"])).unwrap();
        
        scheduler.build().unwrap();
        
        scheduler.fail_node("a", Some("Error")).unwrap();
        
        let skipped = scheduler.skip_dependents("a", "Dependency failed");
        
        assert_eq!(skipped.len(), 3);
        assert!(skipped.contains(&"b".to_string()));
        assert!(skipped.contains(&"c".to_string()));
        assert!(skipped.contains(&"d".to_string()));
        
        assert_eq!(scheduler.get_node("b").unwrap().status, NodeStatus::Skipped);
        assert_eq!(scheduler.get_node("c").unwrap().status, NodeStatus::Skipped);
        assert_eq!(scheduler.get_node("d").unwrap().status, NodeStatus::Skipped);
    }
    
    #[test]
    fn test_critical_path() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("d", "Task D").with_dependencies(&["b", "c"])).unwrap();
        
        scheduler.build().unwrap();
        
        let path = scheduler.critical_path().unwrap();
        assert_eq!(path.len(), 3);
    }
    
    #[test]
    fn test_priority_ordering() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A").with_priority(1)).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B").with_priority(3)).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C").with_priority(2)).unwrap();
        
        scheduler.build().unwrap();
        
        let order = scheduler.topological_sort().unwrap();
        
        assert_eq!(order[0], "b");
        assert_eq!(order[1], "c");
        assert_eq!(order[2], "a");
    }

    #[test]
    fn test_stats() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B").with_dependency("a")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C").with_dependency("a")).unwrap();
        
        scheduler.build().unwrap();
        
        let stats = scheduler.stats();
        
        assert_eq!(stats.total_nodes, 3);
        assert_eq!(stats.total_dependencies, 2);
        assert_eq!(stats.max_depth, 1);
        assert_eq!(stats.pending_count, 3);
    }
    
    #[test]
    fn test_get_ready_nodes_with_max_parallelism() {
        let mut scheduler = DagScheduler::with_max_parallelism(2);
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B")).unwrap();
        scheduler.add_node(DagNode::new("c", "Task C")).unwrap();
        
        scheduler.build().unwrap();
        
        let completed: HashSet<&str> = HashSet::new();
        
        // With 0 running, should get 2 nodes (max parallelism)
        let ready = scheduler.get_ready_nodes(&completed, 0);
        assert_eq!(ready.len(), 2);
        
        // With 1 running, should get 1 node
        let ready = scheduler.get_ready_nodes(&completed, 1);
        assert_eq!(ready.len(), 1);
        
        // With 2 running, should get 0 nodes
        let ready = scheduler.get_ready_nodes(&completed, 2);
        assert!(ready.is_empty());
    }
    
    #[test]
    fn test_from_team() {
        let mut team = AgentTeam::new("team-1", "Test task");
        
        let agent1 = SubAgent::new("agent-1", "Task 1");
        let agent2 = SubAgent::new("agent-2", "Task 2").with_dependency("agent-1");
        let agent3 = SubAgent::new("agent-3", "Task 3").with_dependency("agent-1");
        
        team.add_agent(agent1);
        team.add_agent(agent2);
        team.add_agent(agent3);
        
        let scheduler = DagScheduler::from_team(&team).unwrap();
        
        assert_eq!(scheduler.node_count(), 3);
        
        let waves = scheduler.execution_waves().unwrap();
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0].nodes, vec!["agent-1"]);
        assert_eq!(waves[1].nodes.len(), 2);
    }
    
    #[test]
    fn test_is_complete_and_successful() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B")).unwrap();
        
        scheduler.build().unwrap();
        
        assert!(!scheduler.is_complete());
        assert!(!scheduler.is_successful());
        
        scheduler.complete_node("a").unwrap();
        assert!(!scheduler.is_complete());
        assert!(!scheduler.is_successful());
        
        scheduler.complete_node("b").unwrap();
        assert!(scheduler.is_complete());
        assert!(scheduler.is_successful());
        
        scheduler.fail_node("a", Some("Error")).unwrap();
        assert!(scheduler.is_complete());
        assert!(!scheduler.is_successful());
        assert!(scheduler.has_failures());
    }
    
    #[test]
    fn test_to_dot() {
        let mut scheduler = DagScheduler::new();
        
        scheduler.add_node(DagNode::new("a", "Task A")).unwrap();
        scheduler.add_node(DagNode::new("b", "Task B").with_dependency("a")).unwrap();
        
        scheduler.build().unwrap();
        
        let dot = scheduler.to_dot();
        
        assert!(dot.contains("digraph DAG"));
        assert!(dot.contains("\"a\""));
        assert!(dot.contains("\"b\""));
        assert!(dot.contains("\"a\" -> \"b\""));
    }
}
