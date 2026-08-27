//! Integration tests for multi-agent DAG orchestration.
//!
//! Tests the full pipeline:
//!   1. Load DAG JSON config → deserialize into AgentTeam
//!   2. Build DagScheduler → validate DAG structure
//!   3. Topological sort → verify execution order
//!   4. Execution waves → verify parallel-ready groupings
//!   5. Dependency resolution → verify ready agents API
//!
//! Test fixture: `tests/fixtures/software_audit_dag.json`
//!   - 7 agents: security-scan, style-check, arch-review, perf-bench,
//!     dep-scan (depends on security-scan),
//!     merge-report (depends on all 5),
//!     final-doc (depends on merge-report)
//!   - budget: $5.00, 500K tokens, 30min timeout

use baoclaw_core::engine::team::scheduler::{DagNode, DagScheduler};
use baoclaw_core::engine::team::types::{AgentTeam, TeamMode};
use std::collections::HashSet;

/// Load the software audit DAG from the JSON fixture.
fn load_audit_dag() -> AgentTeam {
    let json = include_str!("fixtures/software_audit_dag.json");
    serde_json::from_str::<AgentTeam>(json).expect("Failed to parse DAG JSON fixture")
}

// ── JSON Fixture Tests ──────────────────────────────────────────────

#[test]
fn test_load_dag_json_fixture() {
    let team = load_audit_dag();
    assert_eq!(team.id, "dag-software-audit-001");
    assert_eq!(team.mode, TeamMode::Dag);
    assert_eq!(team.agents.len(), 7);
    assert_eq!(team.name.as_deref(), Some("Software Code Audit Pipeline"));
    assert!(team.budget.is_some());
    let budget = team.budget.as_ref().unwrap();
    assert_eq!(budget.max_cost_usd, Some(5.0));
    assert_eq!(budget.max_tokens, Some(500_000));
    assert_eq!(budget.max_time_secs, Some(1800));
}

#[test]
fn test_dag_agents_have_correct_dependencies() {
    let team = load_audit_dag();

    // 4 independent root agents (no deps)
    let security = team.get_agent("security-scan").unwrap();
    let style = team.get_agent("style-check").unwrap();
    let arch = team.get_agent("arch-review").unwrap();
    let perf = team.get_agent("perf-bench").unwrap();
    assert!(security.dependencies.is_empty());
    assert!(style.dependencies.is_empty());
    assert!(arch.dependencies.is_empty());
    assert!(perf.dependencies.is_empty());

    // dep-scan depends on security-scan
    let dep_scan = team.get_agent("dep-scan").unwrap();
    assert_eq!(dep_scan.dependencies, vec!["security-scan"]);

    // merge-report depends on all 5 previous agents
    let merge = team.get_agent("merge-report").unwrap();
    assert_eq!(
        merge
            .dependencies
            .iter()
            .map(|s| s.as_str())
            .collect::<HashSet<_>>(),
        vec![
            "security-scan",
            "style-check",
            "arch-review",
            "perf-bench",
            "dep-scan"
        ]
        .into_iter()
        .collect::<HashSet<_>>()
    );

    // final-doc depends only on merge-report
    let final_doc = team.get_agent("final-doc").unwrap();
    assert_eq!(final_doc.dependencies, vec!["merge-report"]);
}

// ── DagScheduler from JSON ──────────────────────────────────────────

#[test]
fn test_dag_scheduler_from_json() {
    let team = load_audit_dag();
    let mut scheduler = DagScheduler::from_team(&team).unwrap();
    // Build validates the DAG
    scheduler.build().unwrap();
    assert_eq!(scheduler.node_count(), 7);
}

#[test]
fn test_dag_scheduler_from_json_is_valid() {
    let team = load_audit_dag();
    let mut scheduler = DagScheduler::from_team(&team).unwrap();
    assert!(scheduler.validate().is_ok());
}

#[test]
fn test_dag_scheduler_topological_sort_from_json() {
    let team = load_audit_dag();
    let mut scheduler = DagScheduler::from_team(&team).unwrap();
    scheduler.build().unwrap();

    let sorted = scheduler.topological_sort().unwrap();
    assert_eq!(sorted.len(), 7);

    // Verify dependency order: every dependency appears before its dependent
    let positions: HashSet<(&str, usize)> = sorted
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    // merge-report must come after all its dependencies
    let merge_pos = positions
        .iter()
        .find(|(id, _)| *id == "merge-report")
        .unwrap()
        .1;
    for dep in &[
        "security-scan",
        "style-check",
        "arch-review",
        "perf-bench",
        "dep-scan",
    ] {
        let dep_pos = positions.iter().find(|(id, _)| id == dep).unwrap().1;
        assert!(
            dep_pos < merge_pos,
            "merge-report depends on {} but appears before it ({} vs {})",
            dep,
            dep_pos,
            merge_pos
        );
    }

    // final-doc must come after merge-report
    let final_pos = positions
        .iter()
        .find(|(id, _)| *id == "final-doc")
        .unwrap()
        .1;
    assert!(merge_pos < final_pos);
}

// ── Execution Waves ──────────────────────────────────────────────────

#[test]
fn test_dag_execution_waves_from_json() {
    let team = load_audit_dag();
    let mut scheduler = DagScheduler::from_team(&team).unwrap();
    scheduler.build().unwrap();

    let waves = scheduler.execution_waves().unwrap();

    // Expected: 4 waves
    // Wave 0: {security-scan, style-check, arch-review, perf-bench} — 4 parallel
    // Wave 1: {dep-scan} — depends on security-scan
    // Wave 2: {merge-report} — depends on all 5
    // Wave 3: {final-doc} — depends on merge-report

    assert_eq!(waves.len(), 4);

    // Wave 0: all 4 root nodes
    let wave0_nodes: HashSet<&str> = waves[0].nodes.iter().map(|s| s.as_str()).collect();
    assert!(wave0_nodes.contains("security-scan"));
    assert!(wave0_nodes.contains("style-check"));
    assert!(wave0_nodes.contains("arch-review"));
    assert!(wave0_nodes.contains("perf-bench"));
    assert!(!wave0_nodes.contains("dep-scan"));
    assert!(!wave0_nodes.contains("merge-report"));
    assert!(!wave0_nodes.contains("final-doc"));

    // Wave 1: dep-scan only
    assert_eq!(waves[1].nodes, vec!["dep-scan"]);

    // Wave 2: merge-report only
    assert_eq!(waves[2].nodes, vec!["merge-report"]);

    // Wave 3: final-doc only
    assert_eq!(waves[3].nodes, vec!["final-doc"]);
}

// ── Ready Agents API (AgentTeam DAG mode) ──────────────────────────

#[test]
fn test_agent_team_ready_agents_in_dag_mode() {
    let mut team = load_audit_dag();

    // Initially: 4 root agents are ready (no dependencies)
    let ready = team.ready_agents();
    assert_eq!(ready.len(), 4);
    let ready_ids: Vec<&str> = ready.iter().map(|a| a.id.as_str()).collect();
    assert!(ready_ids.contains(&"security-scan"));
    assert!(ready_ids.contains(&"style-check"));
    assert!(ready_ids.contains(&"arch-review"));
    assert!(ready_ids.contains(&"perf-bench"));
    assert!(!ready_ids.contains(&"dep-scan"));
    assert!(!ready_ids.contains(&"merge-report"));
    assert!(!ready_ids.contains(&"final-doc"));

    // Mark security-scan as completed → dep-scan should become ready
    let security = team.get_agent_mut("security-scan").unwrap();
    security.complete("Security scan complete".into(), 100, 0.01);
    let ready2 = team.ready_agents();
    assert!(ready2.iter().any(|a| a.id == "dep-scan"));

    // But merge-report still needs all 5 → not ready yet
    assert!(!ready2.iter().any(|a| a.id == "merge-report"));
}

#[test]
fn test_agent_team_dag_all_ready_after_dependencies_complete() {
    let mut team = load_audit_dag();

    // Complete all 5 first-wave agents
    for id in &[
        "security-scan",
        "style-check",
        "arch-review",
        "perf-bench",
        "dep-scan",
    ] {
        let agent = team.get_agent_mut(id).unwrap();
        agent.complete(format!("{} done", id), 50, 0.005);
    }

    // Now merge-report should be ready (all 5 deps completed)
    let ready = team.ready_agents();
    let ready_ids: Vec<&str> = ready.iter().map(|a| a.id.as_str()).collect();
    assert!(ready_ids.contains(&"merge-report"));
    assert!(!ready_ids.contains(&"final-doc"));

    // Complete merge-report → final-doc becomes ready
    let merge = team.get_agent_mut("merge-report").unwrap();
    merge.complete("Merge done".into(), 200, 0.02);

    let ready3 = team.ready_agents();
    assert!(ready3.iter().any(|a| a.id == "final-doc"));
}

#[test]
fn test_agent_team_dag_results_collection() {
    let mut team = load_audit_dag();

    // Complete all agents
    for id in &["security-scan", "style-check", "arch-review", "perf-bench"] {
        team.get_agent_mut(id)
            .unwrap()
            .complete(format!("{} done", id), 50, 0.005);
    }
    team.get_agent_mut("dep-scan")
        .unwrap()
        .complete("Dependencies audited".into(), 30, 0.003);
    team.get_agent_mut("merge-report")
        .unwrap()
        .complete("Reports merged".into(), 100, 0.01);
    team.get_agent_mut("final-doc")
        .unwrap()
        .complete("Final document generated".into(), 200, 0.02);

    // All 7 agents completed
    let results = team.collect_results();
    assert_eq!(results.len(), 7);
    assert_eq!(
        results.get("final-doc"),
        Some(&"Final document generated".to_string())
    );
}

// ── Summary / Budget ─────────────────────────────────────────────────

#[test]
fn test_dag_team_summary() {
    let mut team = load_audit_dag();

    // Complete 3 of 7 agents, skip 1
    team.get_agent_mut("security-scan")
        .unwrap()
        .complete("OK".into(), 100, 0.01);
    team.get_agent_mut("style-check")
        .unwrap()
        .complete("OK".into(), 80, 0.008);
    team.get_agent_mut("arch-review")
        .unwrap()
        .fail("Timeout".into());
    team.get_agent_mut("dep-scan")
        .unwrap()
        .skip("dependency failed".into());
    team.calculate_totals();

    let summary = team.summary();
    assert_eq!(summary.total_agents, 7);
    assert_eq!(summary.completed_count, 2);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.skipped_count, 1);
    assert_eq!(summary.pending_count, 3);
    assert_eq!(summary.total_tokens, 180);
    assert!((summary.total_cost_usd - 0.018).abs() < 0.001);
}

#[test]
fn test_dag_budget_not_exceeded() {
    let team = load_audit_dag();
    // Budget: $5.00 — with zero cost so far, not exceeded
    assert!(!team.is_budget_exceeded());
}

#[test]
fn test_dag_budget_exceeded_after_heavy_usage() {
    let mut team = load_audit_dag();
    let budget = team.budget.as_ref().unwrap();
    assert_eq!(budget.max_cost_usd, Some(5.0));

    // Simulate heavy cost
    for id in &["security-scan", "style-check", "arch-review", "perf-bench"] {
        team.get_agent_mut(id)
            .unwrap()
            .complete("done".into(), 100_000, 1.5);
    }
    team.calculate_totals();
    assert!(team.is_budget_exceeded());
}

// ── Cycle Detection ──────────────────────────────────────────────────

#[test]
fn test_dag_cycle_detection_rejects_cycle() {
    use baoclaw_core::engine::team::scheduler::DagScheduler;

    let mut scheduler = DagScheduler::new();
    scheduler
        .add_node(DagNode::new("a", "Task A").with_dependency("c"))
        .unwrap();
    scheduler
        .add_node(DagNode::new("b", "Task B").with_dependency("a"))
        .unwrap();
    scheduler
        .add_node(DagNode::new("c", "Task C").with_dependency("b"))
        .unwrap();

    let result = scheduler.build();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.code.contains("cycle") || err.message.to_lowercase().contains("cycle"),
        "Expected cycle error, got: {:?}",
        err
    );
}

#[test]
fn test_dag_self_dependency_rejected() {
    use baoclaw_core::engine::team::scheduler::DagScheduler;

    let mut scheduler = DagScheduler::new();
    scheduler
        .add_node(DagNode::new("a", "Task A").with_dependency("a"))
        .unwrap();

    let result = scheduler.build();
    assert!(result.is_err());
}

// ── Priority ─────────────────────────────────────────────────────────

#[test]
fn test_dag_priority_ordering_in_waves() {
    use baoclaw_core::engine::team::scheduler::{DagNode, DagScheduler};

    let mut scheduler = DagScheduler::new();
    // All same wave but different priorities
    scheduler
        .add_node(DagNode::new("low", "Low").with_priority(0))
        .unwrap();
    scheduler
        .add_node(DagNode::new("medium", "Medium").with_priority(5))
        .unwrap();
    scheduler
        .add_node(DagNode::new("high", "High").with_priority(10))
        .unwrap();
    scheduler.build().unwrap();

    let waves = scheduler.execution_waves().unwrap();
    assert_eq!(waves.len(), 1);
    // High priority should come first
    assert_eq!(waves[0].nodes[0], "high");
    assert_eq!(waves[0].nodes[1], "medium");
    assert_eq!(waves[0].nodes[2], "low");
}

// ── Missing Dependency ──────────────────────────────────────────────

#[test]
fn test_dag_missing_dependency_error() {
    use baoclaw_core::engine::team::scheduler::DagScheduler;

    let mut scheduler = DagScheduler::new();
    scheduler
        .add_node(DagNode::new("agent-1", "Task 1").with_dependency("non-existent"))
        .unwrap();

    let result = scheduler.build();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.code.contains("missing_dependency"));
}

// ── to_dot / Visualization ──────────────────────────────────────────

#[test]
fn test_dag_to_dot_output() {
    let team = load_audit_dag();
    let mut scheduler = DagScheduler::from_team(&team).unwrap();
    scheduler.build().unwrap();

    let dot = scheduler.to_dot();
    assert!(dot.starts_with("digraph"));
    // Should contain all nodes
    for id in &[
        "security-scan",
        "style-check",
        "arch-review",
        "perf-bench",
        "dep-scan",
        "merge-report",
        "final-doc",
    ] {
        assert!(dot.contains(id), "DOT output missing node: {}", id);
    }
    // Should contain edges for dependencies
    assert!(dot.contains("\"security-scan\" -> \"merge-report\""));
    assert!(dot.contains("\"dep-scan\" -> \"merge-report\""));
    assert!(dot.contains("\"merge-report\" -> \"final-doc\""));
}
