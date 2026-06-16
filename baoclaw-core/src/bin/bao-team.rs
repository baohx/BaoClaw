//! bao-team — BaoClaw Multi-Agent Team Runner
//!
//! CLI for validating and executing DAG-based multi-agent workflows.
//!
//! ## Usage
//!
//! ```bash
//! # Build the binary
//! cargo build --release --bin bao-team
//!
//! # Validate a DAG file (no API key needed)
//! ./target/release/bao-team validate my_workflow.json
//!
//! # Generate DOT graph for visualization
//! ./target/release/bao-team dot my_workflow.json > graph.dot
//! dot -Tpng graph.dot -o graph.png
//!
//! # Execute a DAG with LLM (requires API key via config)
//! ./target/release/bao-team run my_workflow.json
//! ```

use std::path::Path;

use baoclaw_core::engine::team::scheduler::DagScheduler;
use baoclaw_core::engine::team::types::{AgentTeam, TeamMode};

const USAGE: &str = r#"
bao-team — BaoClaw Multi-Agent Team Runner

USAGE:
    bao-team <command> <dag-file.json> [options]

COMMANDS:
    validate   Load DAG JSON, validate structure, print execution plan
    dot        Generate DOT graph output (for Graphviz visualization)
    run        Execute DAG with LLM agents (requires `baoclaw-daemon` running)

EXAMPLES:
    bao-team validate my_workflow.json
    bao-team dot my_workflow.json | dot -Tpng -o graph.png
    bao-team run my_workflow.json

Get started:
    1. Copy tests/fixtures/software_audit_dag.json as a template
    2. Edit your agents, prompts, and dependencies
    3. Run `bao-team validate your_workflow.json` to verify
"#;

fn load_dag(path: &str) -> Result<AgentTeam, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Cannot read '{}': {}", path, e))?;
    let team: AgentTeam =
        serde_json::from_str(&content).map_err(|e| format!("Invalid DAG JSON: {}", e))?;
    if team.mode != TeamMode::Dag {
        return Err(format!(
            "Expected mode 'dag', got '{}'. This CLI only supports DAG mode.",
            team.mode
        ));
    }
    Ok(team)
}

fn cmd_validate(dag_path: &str) -> Result<(), String> {
    let team = load_dag(dag_path)?;
    let dag_name = team
        .name
        .as_deref()
        .unwrap_or(&team.id);

    println!("═══ DAG: {} ═══", dag_name);
    println!("Task: {}", team.task);
    println!("  Agents: {}", team.agents.len());
    if let Some(b) = &team.budget {
        println!(
            "  Budget: ${:.2} | {} tokens | {}s timeout",
            b.max_cost_usd.unwrap_or(f64::INFINITY),
            b.max_tokens.unwrap_or(u64::MAX),
            b.max_time_secs.unwrap_or(0),
        );
    }

    // Build scheduler & validate
    let mut scheduler = DagScheduler::from_team(&team)
        .map_err(|e| format!("Scheduler error: {}", e))?;
    scheduler
        .build()
        .map_err(|e| format!("Invalid DAG: {}", e))?;

    println!("✅ DAG structure valid — no cycles, all dependencies satisfied.");

    // Topological sort
    let sorted = scheduler.topological_sort().map_err(|e| e.to_string())?;
    println!("\n📋 Execution order (topological):");
    for (i, id) in sorted.iter().enumerate() {
        let agent = team.get_agent(id).unwrap_or_else(|| panic!("Missing agent: {}", id));
        let dep_str = if agent.dependencies.is_empty() {
            "(root)".to_string()
        } else {
            format!("← {}", agent.dependencies.join(", "))
        };
        println!("  {}. {} {}", i + 1, id, dep_str);
    }

    // Execution waves
    let waves = scheduler.execution_waves().map_err(|e| e.to_string())?;
    println!("\n🌊 Execution waves:");
    for wave in &waves {
        let label = if wave.parallel { "parallel" } else { "sequential" };
        println!(
            "  Wave {} ({}): {}",
            wave.wave,
            label,
            wave.nodes.join(", ")
        );
    }

    // Critical path (longest dependency chain)
    let critical = scheduler
        .critical_path()
        .map_err(|e| e.to_string())?;
    println!("\n🔴 Critical path (longest chain): {} steps", critical.len());
    println!("   {}", critical.join(" → "));

    // Agent detail
    println!("\n📦 Agent details:");
    for agent in &team.agents {
        let deps = if agent.dependencies.is_empty() {
            "none".to_string()
        } else {
            agent.dependencies.join(", ")
        };
        println!("  [{}]", agent.id);
        println!("    Name:    {}", agent.name);
        println!("    Deps:    {}", deps);
        // Show first 80 chars of prompt
        let preview: String = agent.prompt.chars().take(80).collect();
        println!("    Prompt:  {}...", preview);
        println!();
    }

    Ok(())
}

fn cmd_dot(dag_path: &str) -> Result<(), String> {
    let team = load_dag(dag_path)?;
    let mut scheduler = DagScheduler::from_team(&team)
        .map_err(|e| format!("Scheduler error: {}", e))?;
    scheduler
        .build()
        .map_err(|e| format!("Invalid DAG: {}", e))?;
    println!("{}", scheduler.to_dot());
    Ok(())
}

fn cmd_run(dag_path: &str) -> Result<(), String> {
    let team = load_dag(dag_path)?;
    let mut scheduler = DagScheduler::from_team(&team)
        .map_err(|e| format!("Scheduler error: {}", e))?;
    scheduler
        .build()
        .map_err(|e| format!("Invalid DAG: {}", e))?;

    let waves = scheduler.execution_waves().map_err(|e| e.to_string())?;
    let dag_name = team.name.as_deref().unwrap_or(&team.id);

    println!("═══ Executing DAG: {} ═══", dag_name);
    println!("  Agents: {} | Waves: {} | Budget: {:?}", 
        team.agents.len(), waves.len(), team.budget);

    println!("\n⚠️  'bao-team run' requires the full BaoClaw daemon infrastructure");
    println!("    (API client, tools, permission gate, etc.).");
    println!();
    println!("This is a placeholder. In production, this would:");
    for wave in &waves {
        println!(
            "  Wave {}: run {} sub-agent(s) → wait → collect results",
            wave.wave,
            wave.nodes.len()
        );
    }
    println!();
    println!("🔧 To run with API: pass this DAG to the running baoclaw daemon");
    println!("   via its IPC interface using the team manager.");
    println!();
    println!("✅ DAG validation passed. Workflow is ready for execution.");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        println!("{}", USAGE);
        std::process::exit(1);
    }

    let command = &args[1];
    let dag_path = &args[2];

    if !Path::new(dag_path).exists() {
        eprintln!("Error: file '{}' not found.", dag_path);
        std::process::exit(1);
    }

    let result = match command.as_str() {
        "validate" => cmd_validate(dag_path),
        "dot" => cmd_dot(dag_path),
        "run" => cmd_run(dag_path),
        _ => {
            eprintln!("Unknown command: '{}'\n{}", command, USAGE);
            std::process::exit(1);
        }
    };

    match result {
        Ok(()) => {
            println!("\n✨ Done.");
        }
        Err(e) => {
            eprintln!("\n❌ Error: {}", e);
            std::process::exit(1);
        }
    }
}
