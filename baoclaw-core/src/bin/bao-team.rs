#![allow(dead_code)]
//! bao-team — BaoClaw Multi-Agent Team Runner
//!
//! CLI for validating and executing DAG-based multi-agent workflows,
//! with natural-language intent matching.
//!
//! ## Usage
//!
//! ```bash
//! # Natural language — just say what you want
//! ./target/release/bao-team match "帮我代码审查"
//!
//! # Interactive REPL
//! ./target/release/bao-team repl
//!
//! # Direct commands
//! ./target/release/bao-team validate my_workflow.json
//! ./target/release/bao-team dot my_workflow.json | dot -Tpng -o graph.png
//! ```

use std::path::Path;

use baoclaw_core::engine::team::scheduler::DagScheduler;
use baoclaw_core::engine::team::types::{AgentTeam, TeamMode};

// ── DAG Registry Types ─────────────────────────────────────────

#[derive(serde::Deserialize, Clone, Debug)]
struct DagRegistryEntry {
    intent: String,
    dag_file: String,
    display_name: String,
    description: String,
    trigger_phrases: Vec<String>,
    keywords: Vec<String>,
    expected_duration: String,
    estimated_cost: String,
}

#[derive(serde::Deserialize)]
struct DagRegistry {
    entries: Vec<DagRegistryEntry>,
}

/// Match result with confidence score
struct MatchResult {
    entry: DagRegistryEntry,
    score: f64,
    matched_phrases: Vec<String>,
}

fn load_registry() -> Result<Vec<DagRegistryEntry>, String> {
    // Try to find dag_registry.json next to the DAG fixtures
    let candidates = [
        "tests/fixtures/dag_registry.json",
        "baoclaw-core/tests/fixtures/dag_registry.json",
    ];
    let content = candidates
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .ok_or_else(|| "Cannot find dag_registry.json (searched tests/fixtures/).".to_string())?;
    let entries: Vec<DagRegistryEntry> =
        serde_json::from_str(&content).map_err(|e| format!("Invalid registry: {}", e))?;
    Ok(entries)
}

/// Match natural language input against the DAG registry.
/// Returns results sorted by score (highest first).
fn match_intent(input: &str, registry: &[DagRegistryEntry]) -> Vec<MatchResult> {
    let input_lower = input.to_lowercase();
    let mut results: Vec<MatchResult> = Vec::new();

    for entry in registry {
        let mut score = 0.0_f64;
        let mut matched_phrases: Vec<String> = Vec::new();

        // 1. Exact trigger phrase match (high weight)
        for phrase in &entry.trigger_phrases {
            if input_lower.contains(&phrase.to_lowercase()) {
                score += 3.0;
                matched_phrases.push(phrase.clone());
            }
        }

        // 2. Keyword match (medium weight)
        for kw in &entry.keywords {
            if input_lower.contains(&kw.to_lowercase()) {
                score += 1.0;
                matched_phrases.push(kw.clone());
            }
        }

        // 3. Description fuzzy match — bonus for intent words in description
        let desc_lower = entry.description.to_lowercase();
        for word in input_lower.split_whitespace() {
            if word.len() >= 2 && desc_lower.contains(word) {
                score += 0.5;
            }
        }

        if score > 0.0 {
            results.push(MatchResult {
                entry: entry.clone(),
                score,
                matched_phrases,
            });
        }
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    results
}

fn resolve_dag_path(dag_file: &str) -> Result<String, String> {
    let candidates = vec![
        format!("tests/fixtures/{}", dag_file),
        format!("baoclaw-core/tests/fixtures/{}", dag_file),
        dag_file.to_string(),
    ];
    for p in &candidates {
        if Path::new(p).exists() {
            return Ok(p.clone());
        }
    }
    Err(format!(
        "Cannot find DAG file '{}' (searched: {:?})",
        dag_file, candidates
    ))
}

// ── Commands ───────────────────────────────────────────────────

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

fn cmd_match(input: &str) -> Result<(), String> {
    let registry = load_registry()?;
    let results = match_intent(input, &registry);

    if results.is_empty() {
        println!("🤔 No matching DAG workflow found for: \"{}\"", input);
        println!();
        println!("Available workflows:");
        for entry in &registry {
            println!("  • {} ({})", entry.display_name, entry.intent);
        }
        return Ok(());
    }

    let best = &results[0];
    let confidence_pct = if best.score >= 6.0 {
        "high"
    } else if best.score >= 3.0 {
        "medium"
    } else {
        "low"
    };

    println!("╔══════════════════════════════════════════════╗");
    println!(
        "║  🔍 Intent: {} (confidence: {})",
        best.entry.intent, confidence_pct
    );
    println!("╠══════════════════════════════════════════════╣");
    println!("║  {}", best.entry.display_name);
    println!("║  {}", best.entry.description);
    println!(
        "║  Duration: {} | Cost: {}",
        best.entry.expected_duration, best.entry.estimated_cost
    );
    println!("║  Matched: {}", best.matched_phrases.join(", "));
    println!("╚══════════════════════════════════════════════╝");
    println!();

    // If there are other good matches, show them too
    if results.len() > 1 && results[1].score >= best.score * 0.5 {
        println!("Other possibilities:");
        for r in &results[1..results.len().min(3)] {
            if r.score >= 1.0 {
                println!("  · {} (score: {:.1})", r.entry.display_name, r.score);
            }
        }
        println!();
    }

    // Validate the DAG
    let dag_path = resolve_dag_path(&best.entry.dag_file)?;
    let team = load_dag(&dag_path)?;
    let mut scheduler =
        DagScheduler::from_team(&team).map_err(|e| format!("Scheduler error: {}", e))?;
    scheduler
        .build()
        .map_err(|e| format!("Invalid DAG: {}", e))?;

    let waves = scheduler.execution_waves().map_err(|e| e.to_string())?;
    println!(
        "✅ DAG valid: {} agents, {} waves",
        team.agents.len(),
        waves.len()
    );
    for (i, wave) in waves.iter().enumerate() {
        println!("   Wave {}: {}", i, wave.nodes.join(" → "));
    }

    let critical = scheduler.critical_path().map_err(|e| e.to_string())?;
    println!("   Critical: {}", critical.join(" → "));

    println!();
    println!("📋 To execute:");
    println!("   bao-team validate {}", dag_path);
    println!("   bao-team run {}", dag_path);

    Ok(())
}

fn cmd_repl() -> Result<(), String> {
    let registry = load_registry()?;

    println!("╔══════════════════════════════════════════════╗");
    println!("║  🤖 BaoClaw Multi-Agent Team REPL            ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  Describe what you want in natural language. ║");
    println!("║  Type /help, /list, or /quit.                ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();

    let stdin = std::io::stdin();
    let mut line = String::new();

    loop {
        line.clear();
        print!("baoclaw> ");
        use std::io::Write;
        std::io::stdout().flush().unwrap();

        match stdin.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }

        let input = line.trim();

        if input.is_empty() {
            continue;
        }

        match input {
            "/quit" | "/exit" | "/q" => {
                println!("Bye!");
                break;
            }
            "/help" | "/h" => {
                println!("Commands:");
                println!("  /help, /h     — this message");
                println!("  /list, /l     — list available DAG workflows");
                println!("  /quit, /q     — exit");
                println!("  anything else  — match against DAG registry");
                println!();
                continue;
            }
            "/list" | "/l" => {
                println!("Available DAG workflows:");
                for entry in &registry {
                    println!("  {} — {}", entry.display_name, entry.description);
                }
                println!();
                continue;
            }
            _ => {
                // Treat as natural language intent
                match cmd_match(input) {
                    Ok(()) => println!(),
                    Err(e) => eprintln!("Error: {}\n", e),
                }
            }
        }
    }

    Ok(())
}

fn cmd_validate(dag_path: &str) -> Result<(), String> {
    let team = load_dag(dag_path)?;
    let dag_name = team.name.as_deref().unwrap_or(&team.id);

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

    let mut scheduler =
        DagScheduler::from_team(&team).map_err(|e| format!("Scheduler error: {}", e))?;
    scheduler
        .build()
        .map_err(|e| format!("Invalid DAG: {}", e))?;

    println!("✅ DAG structure valid — no cycles, all dependencies satisfied.");

    let sorted = scheduler.topological_sort().map_err(|e| e.to_string())?;
    println!("\n📋 Execution order (topological):");
    for (i, id) in sorted.iter().enumerate() {
        let agent = team.get_agent(id).unwrap();
        let dep_str = if agent.dependencies.is_empty() {
            "(root)".to_string()
        } else {
            format!("← {}", agent.dependencies.join(", "))
        };
        println!("  {}. {} {}", i + 1, id, dep_str);
    }

    let waves = scheduler.execution_waves().map_err(|e| e.to_string())?;
    println!("\n🌊 Execution waves:");
    for wave in &waves {
        let label = if wave.parallel {
            "parallel"
        } else {
            "sequential"
        };
        println!(
            "  Wave {} ({}): {}",
            wave.wave,
            label,
            wave.nodes.join(", ")
        );
    }

    let critical = scheduler.critical_path().map_err(|e| e.to_string())?;
    println!("\n🔴 Critical path: {}", critical.join(" → "));
    println!();

    for agent in &team.agents {
        let deps = if agent.dependencies.is_empty() {
            "none".to_string()
        } else {
            agent.dependencies.join(", ")
        };
        let preview: String = agent.prompt.chars().take(80).collect();
        println!("  [{}] ← {} | {}...", agent.id, deps, preview);
    }

    Ok(())
}

fn cmd_dot(dag_path: &str) -> Result<(), String> {
    let team = load_dag(dag_path)?;
    let mut scheduler =
        DagScheduler::from_team(&team).map_err(|e| format!("Scheduler error: {}", e))?;
    scheduler
        .build()
        .map_err(|e| format!("Invalid DAG: {}", e))?;
    println!("{}", scheduler.to_dot());
    Ok(())
}

fn cmd_run(dag_path: &str) -> Result<(), String> {
    let team = load_dag(dag_path)?;
    let mut scheduler =
        DagScheduler::from_team(&team).map_err(|e| format!("Scheduler error: {}", e))?;
    scheduler
        .build()
        .map_err(|e| format!("Invalid DAG: {}", e))?;

    let waves = scheduler.execution_waves().map_err(|e| e.to_string())?;
    let dag_name = team.name.as_deref().unwrap_or(&team.id);

    println!("═══ Executing DAG: {} ═══", dag_name);
    println!(
        "  Agents: {} | Waves: {} | Budget: {:?}",
        team.agents.len(),
        waves.len(),
        team.budget
    );

    println!("\n⚠️  'bao-team run' requires the full BaoClaw daemon.");
    println!("    For production execution, connect to the running daemon via IPC.");
    println!();
    println!("✅ DAG validation passed. Workflow is ready for execution.");
    Ok(())
}

// ── Main ───────────────────────────────────────────────────────

const USAGE: &str = r#"
bao-team — BaoClaw Multi-Agent Team Runner

USAGE:
    bao-team <command> [args...]

COMMANDS:
    match <text>    Match natural language to DAG workflow
    repl            Interactive REPL (natural language → DAG)
    validate <file> Validate DAG JSON and print execution plan
    dot <file>      Generate DOT graph (for Graphviz)
    run <file>      Execute DAG (requires daemon)

EXAMPLES:
    bao-team match "帮我代码审查"
    bao-team repl
    bao-team validate my_workflow.json
    bao-team dot my_workflow.json | dot -Tpng -o graph.png
"#;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        println!("{}", USAGE);
        std::process::exit(1);
    }

    let command = &args[1];

    let result = match command.as_str() {
        "match" => {
            if args.len() < 3 {
                eprintln!("Usage: bao-team match \"your natural language query\"");
                std::process::exit(1);
            }
            cmd_match(&args[2..].join(" "))
        }
        "repl" => cmd_repl(),
        "validate" | "dot" | "run" => {
            if args.len() < 3 {
                eprintln!("Usage: bao-team {} <dag-file.json>", command);
                std::process::exit(1);
            }
            let dag_path = &args[2];
            if !Path::new(dag_path).exists() && command != "run" {
                eprintln!("Error: file '{}' not found.", dag_path);
                std::process::exit(1);
            }
            match command.as_str() {
                "validate" => cmd_validate(dag_path),
                "dot" => cmd_dot(dag_path),
                "run" => cmd_run(dag_path),
                _ => unreachable!(),
            }
        }
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
