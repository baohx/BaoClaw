//! Context window intelligent allocation — prioritizes context blocks by attention score.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A block of context competing for window space.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextBlock {
    /// Unique identifier.
    pub id: String,
    /// Category of context.
    pub category: ContextCategory,
    /// Estimated token count.
    pub token_count: usize,
    /// Relevance score (0.0-1.0) based on current query.
    pub relevance: f64,
    /// Recency score (0.0-1.0) — how recently was this accessed.
    pub recency: f64,
    /// Frequency score (0.0-1.0) — how often is this referenced.
    pub frequency: f64,
    /// Whether this block is mandatory (system prompt, tools).
    pub mandatory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextCategory {
    SystemPrompt,
    Tools,
    Memory,
    Skills,
    Rules,
    SessionHistory,
    CrossSessionSearch,
    UserProfile,
    DynamicReminder,
    ToolHealthWarnings,
    IntentPrediction,
}

/// Result of context allocation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllocationResult {
    /// Blocks that were included.
    pub included: Vec<ContextBlock>,
    /// Blocks that were trimmed or excluded.
    pub excluded: Vec<ContextBlock>,
    /// Total tokens allocated.
    pub total_tokens: usize,
    /// Budget that was available.
    pub budget: usize,
    /// Utilization ratio.
    pub utilization: f64,
}

/// Manages intelligent allocation of context window space.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextAllocator {
    /// Weights for scoring: (relevance, recency, frequency).
    pub weights: (f64, f64, f64),
    /// Minimum relevance to include optional blocks.
    pub min_relevance: f64,
    /// Historical allocation stats.
    pub stats: AllocationStats,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AllocationStats {
    pub total_allocations: u64,
    pub avg_utilization: f64,
    pub category_usage: HashMap<String, u64>,
    pub avg_tokens_per_category: HashMap<String, f64>,
}

impl ContextAllocator {
    pub fn new() -> Self {
        Self {
            weights: (0.5, 0.3, 0.2), // relevance most important
            min_relevance: 0.1,
            stats: AllocationStats::default(),
        }
    }

    /// Compute attention score for a context block.
    pub fn attention_score(&self, block: &ContextBlock) -> f64 {
        if block.mandatory {
            return 1.0;
        }
        let (wr, wc, wf) = self.weights;
        (wr * block.relevance + wc * block.recency + wf * block.frequency).clamp(0.0, 1.0)
    }

    /// Allocate context window budget across competing blocks.
    ///
    /// Strategy:
    /// 1. Reserve budget for all mandatory blocks.
    /// 2. Score optional blocks by attention.
    /// 3. Fill remaining budget greedily by score.
    /// 4. If budget exceeded, trim lowest-scoring optional blocks.
    pub fn allocate(&mut self, blocks: Vec<ContextBlock>, budget: usize) -> AllocationResult {
        let mut mandatory: Vec<ContextBlock> = Vec::new();
        let mut optional: Vec<ContextBlock> = Vec::new();

        for block in blocks {
            if block.mandatory {
                mandatory.push(block);
            } else {
                optional.push(block);
            }
        }

        // Calculate mandatory cost
        let mandatory_tokens: usize = mandatory.iter().map(|b| b.token_count).sum();
        let mut remaining = budget.saturating_sub(mandatory_tokens);

        // Score and sort optional blocks
        let mut scored: Vec<(f64, ContextBlock)> = optional
            .into_iter()
            .map(|b| (self.attention_score(&b), b))
            .filter(|(score, _)| *score >= self.min_relevance)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Greedy fill
        let mut included = mandatory;
        let mut excluded = Vec::new();

        for (score, block) in scored {
            if block.token_count <= remaining {
                remaining -= block.token_count;
                included.push(block);
            } else {
                excluded.push(block);
            }
        }

        let total_tokens = included.iter().map(|b| b.token_count).sum();
        let utilization = if budget > 0 {
            total_tokens as f64 / budget as f64
        } else {
            0.0
        };

        // Update stats
        self.stats.total_allocations += 1;
        self.stats.avg_utilization = if self.stats.total_allocations == 1 {
            utilization
        } else {
            self.stats.avg_utilization * 0.9 + utilization * 0.1 // EMA
        };
        for block in &included {
            let cat = format!("{:?}", block.category);
            *self.stats.category_usage.entry(cat.clone()).or_insert(0) += 1;
        }

        AllocationResult {
            included,
            excluded,
            total_tokens,
            budget,
            utilization,
        }
    }

    /// Convenience: compute recency score from timestamp.
    /// More recent = higher score. Uses exponential decay.
    pub fn compute_recency(last_accessed_secs_ago: u64) -> f64 {
        // Half-life of 30 minutes (1800 seconds)
        let half_life = 1800.0_f64;
        let ratio = -(last_accessed_secs_ago as f64) / half_life;
        ratio.exp()
    }

    /// Convenience: compute frequency score from usage count.
    pub fn compute_frequency(usage_count: u32, max_observed: u32) -> f64 {
        if max_observed == 0 { return 0.0; }
        (usage_count as f64 / max_observed as f64).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mandatory_blocks_always_included() {
        let mut alloc = ContextAllocator::new();
        let blocks = vec![
            ContextBlock {
                id: "system".into(),
                category: ContextCategory::SystemPrompt,
                token_count: 500,
                relevance: 1.0,
                recency: 1.0,
                frequency: 1.0,
                mandatory: true,
            },
            ContextBlock {
                id: "memory".into(),
                category: ContextCategory::Memory,
                token_count: 200,
                relevance: 0.5,
                recency: 0.8,
                frequency: 0.3,
                mandatory: false,
            },
        ];
        let result = alloc.allocate(blocks, 800);
        assert!(result.included.len() == 2);
        assert_eq!(result.total_tokens, 700);
    }

    #[test]
    fn test_budget_exceeded_trims_low_score() {
        let mut alloc = ContextAllocator::new();
        let blocks = vec![
            ContextBlock {
                id: "mandatory".into(),
                category: ContextCategory::SystemPrompt,
                token_count: 500,
                relevance: 1.0,
                recency: 1.0,
                frequency: 1.0,
                mandatory: true,
            },
            ContextBlock {
                id: "high_score".into(),
                category: ContextCategory::Skills,
                token_count: 300,
                relevance: 0.9,
                recency: 0.8,
                frequency: 0.7,
                mandatory: false,
            },
            ContextBlock {
                id: "low_score".into(),
                category: ContextCategory::CrossSessionSearch,
                token_count: 300,
                relevance: 0.1,
                recency: 0.1,
                frequency: 0.1,
                mandatory: false,
            },
        ];
        // Budget only fits mandatory + one optional
        let result = alloc.allocate(blocks, 850);
        assert!(result.included.len() == 2);
        assert!(result.excluded.len() == 1);
        assert_eq!(result.excluded[0].id, "low_score");
    }

    #[test]
    fn test_attention_score_mandatory_is_one() {
        let alloc = ContextAllocator::new();
        let block = ContextBlock {
            id: "sys".into(),
            category: ContextCategory::SystemPrompt,
            token_count: 100,
            relevance: 0.0,
            recency: 0.0,
            frequency: 0.0,
            mandatory: true,
        };
        assert_eq!(alloc.attention_score(&block), 1.0);
    }

    #[test]
    fn test_recency_decay() {
        let now = ContextAllocator::compute_recency(0);
        assert!(now > 0.99);
        let old = ContextAllocator::compute_recency(3600); // 1 hour ago
        assert!(old < 0.2);
    }
}
