//! Data-only query cost planner for robot metadata.
//!
//! This module does not change search execution. It summarizes the plan cass
//! already chose and the realized outcome so operators can reason about tail
//! cost, fallback tiers, cache behavior, and cursor continuity from `_meta`.

use serde::{Deserialize, Serialize};

pub const QUERY_COST_PLAN_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryCostPlan {
    pub schema_version: String,
    pub planner_id: String,
    pub phases: Vec<QueryPhasePlan>,
    pub budget_exhaustion: Option<BudgetExhaustion>,
    pub result_identity: ResultIdentityContinuity,
    pub cache: CachePlan,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPhasePlan {
    pub phase: QueryPhase,
    pub planned: bool,
    pub realized: bool,
    pub budget: PhaseBudget,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryPhase {
    Lexical,
    Semantic,
    Hydration,
    Output,
    Cursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseBudget {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub max_tokens: Option<usize>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetExhaustion {
    pub kind: BudgetExhaustionKind,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetExhaustionKind {
    TokenBudget,
    Timeout,
    CursorPage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultIdentityContinuity {
    pub input_cursor_present: bool,
    pub next_cursor_present: bool,
    pub cursor_continuation: bool,
    pub offset: usize,
    pub limit: usize,
    pub returned_count: usize,
    pub total_matches: usize,
    pub continuity_key: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachePlan {
    pub eligible: bool,
    pub hits: u64,
    pub misses: u64,
    pub shortfall: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryCostPlanInput {
    pub query_chars: usize,
    pub requested_mode: String,
    pub realized_mode: String,
    pub fallback_tier: Option<String>,
    pub fallback_reason: Option<String>,
    pub semantic_refinement: bool,
    pub wildcard_fallback: bool,
    pub limit: usize,
    pub offset: usize,
    pub returned_count: usize,
    pub total_matches: usize,
    pub max_tokens: Option<usize>,
    pub tokens_estimated: Option<usize>,
    pub hits_clamped: bool,
    pub timeout_ms: Option<u64>,
    pub timed_out: bool,
    pub input_cursor_present: bool,
    pub next_cursor_present: bool,
    pub output_projection: String,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_shortfall: u64,
    pub aggregation_count: usize,
}

pub fn build_query_cost_plan(input: QueryCostPlanInput) -> QueryCostPlan {
    let semantic_requested = input.requested_mode == "semantic" || input.requested_mode == "hybrid";
    let semantic_realized = input.realized_mode == "semantic"
        || input.realized_mode == "hybrid"
        || input.semantic_refinement;
    let lexical_realized = input.realized_mode == "lexical"
        || input.realized_mode == "hybrid"
        || input.wildcard_fallback;
    let budget_exhaustion = budget_exhaustion(&input);
    let result_identity = result_identity_continuity(&input);
    let cache = cache_plan(&input);

    let mut phases = vec![
        QueryPhasePlan {
            phase: QueryPhase::Lexical,
            planned: input.requested_mode != "semantic",
            realized: lexical_realized,
            budget: PhaseBudget {
                limit: Some(input.limit),
                offset: Some(input.offset),
                max_tokens: None,
                timeout_ms: input.timeout_ms,
            },
            reason: lexical_reason(&input, lexical_realized),
        },
        QueryPhasePlan {
            phase: QueryPhase::Semantic,
            planned: semantic_requested,
            realized: semantic_realized,
            budget: PhaseBudget {
                limit: Some(input.limit),
                offset: Some(input.offset),
                max_tokens: None,
                timeout_ms: input.timeout_ms,
            },
            reason: semantic_reason(&input, semantic_realized),
        },
        QueryPhasePlan {
            phase: QueryPhase::Hydration,
            planned: true,
            realized: input.returned_count > 0,
            budget: PhaseBudget {
                limit: Some(input.limit),
                offset: Some(input.offset),
                max_tokens: input.max_tokens,
                timeout_ms: input.timeout_ms,
            },
            reason: format!(
                "hydrated {} result(s) for {} total match(es)",
                input.returned_count, input.total_matches
            ),
        },
        QueryPhasePlan {
            phase: QueryPhase::Output,
            planned: true,
            realized: true,
            budget: PhaseBudget {
                limit: Some(input.limit),
                offset: Some(input.offset),
                max_tokens: input.max_tokens,
                timeout_ms: input.timeout_ms,
            },
            reason: output_reason(&input),
        },
        QueryPhasePlan {
            phase: QueryPhase::Cursor,
            planned: input.next_cursor_present || input.input_cursor_present,
            realized: input.next_cursor_present,
            budget: PhaseBudget {
                limit: Some(input.limit),
                offset: Some(input.offset),
                max_tokens: None,
                timeout_ms: None,
            },
            reason: result_identity.reason.clone(),
        },
    ];
    phases.sort_by_key(|phase| phase.phase as u8);

    QueryCostPlan {
        schema_version: QUERY_COST_PLAN_SCHEMA_VERSION.to_string(),
        planner_id: "query_cost.v1".to_string(),
        phases,
        budget_exhaustion,
        result_identity,
        cache,
        summary: format!(
            "{} mode realized with {} returned / {} total match(es), projection={}, query_chars={}",
            input.realized_mode,
            input.returned_count,
            input.total_matches,
            input.output_projection,
            input.query_chars
        ),
    }
}

fn budget_exhaustion(input: &QueryCostPlanInput) -> Option<BudgetExhaustion> {
    if input.timed_out {
        return Some(BudgetExhaustion {
            kind: BudgetExhaustionKind::Timeout,
            reason: format!(
                "search reported partial results after timeout budget {:?}ms",
                input.timeout_ms
            ),
        });
    }
    if input.hits_clamped {
        return Some(BudgetExhaustion {
            kind: BudgetExhaustionKind::TokenBudget,
            reason: format!(
                "output was clamped to max_tokens={:?} after estimating {:?} tokens",
                input.max_tokens, input.tokens_estimated
            ),
        });
    }
    if input.next_cursor_present {
        return Some(BudgetExhaustion {
            kind: BudgetExhaustionKind::CursorPage,
            reason: "result window ended before the full match set; continue with next_cursor"
                .to_string(),
        });
    }
    None
}

fn result_identity_continuity(input: &QueryCostPlanInput) -> ResultIdentityContinuity {
    let cursor_continuation = input.input_cursor_present || input.next_cursor_present;
    let reason = if input.input_cursor_present && input.next_cursor_present {
        "continued an existing cursor and emitted the next page cursor"
    } else if input.input_cursor_present {
        "continued an existing cursor and exhausted the visible result window"
    } else if input.next_cursor_present {
        "first page preserved continuity by emitting next_cursor"
    } else {
        "single response contains the visible result identity window"
    };
    ResultIdentityContinuity {
        input_cursor_present: input.input_cursor_present,
        next_cursor_present: input.next_cursor_present,
        cursor_continuation,
        offset: input.offset,
        limit: input.limit,
        returned_count: input.returned_count,
        total_matches: input.total_matches,
        continuity_key: format!(
            "offset:{}:limit:{}:returned:{}:total:{}",
            input.offset, input.limit, input.returned_count, input.total_matches
        ),
        reason: reason.to_string(),
    }
}

fn cache_plan(input: &QueryCostPlanInput) -> CachePlan {
    let eligible = input.aggregation_count == 0 && input.max_tokens.is_none();
    let reason = if !eligible && input.aggregation_count > 0 {
        "aggregation query bypasses reusable hit-cache admission"
    } else if !eligible {
        "token-budgeted output bypasses reusable hit-cache admission"
    } else if input.cache_hits > 0 {
        "cache supplied at least one hit"
    } else if input.cache_misses > 0 {
        "cache was eligible but missed"
    } else {
        "cache eligible; no cache event was reported"
    };
    CachePlan {
        eligible,
        hits: input.cache_hits,
        misses: input.cache_misses,
        shortfall: input.cache_shortfall,
        reason: reason.to_string(),
    }
}

fn lexical_reason(input: &QueryCostPlanInput, realized: bool) -> String {
    if realized && input.wildcard_fallback {
        "lexical phase realized with wildcard fallback".to_string()
    } else if realized {
        "lexical phase realized for the selected search mode".to_string()
    } else {
        "lexical phase skipped because semantic-only mode was realized".to_string()
    }
}

fn semantic_reason(input: &QueryCostPlanInput, realized: bool) -> String {
    if realized {
        "semantic phase realized for semantic or hybrid search".to_string()
    } else if let Some(reason) = &input.fallback_reason {
        format!(
            "semantic phase planned but fell back to {:?}: {reason}",
            input.fallback_tier
        )
    } else if input.requested_mode == "lexical" {
        "semantic phase not planned for lexical mode".to_string()
    } else {
        "semantic phase was not realized".to_string()
    }
}

fn output_reason(input: &QueryCostPlanInput) -> String {
    if input.hits_clamped {
        format!(
            "projection {} was clamped by max_tokens={:?}",
            input.output_projection, input.max_tokens
        )
    } else {
        format!(
            "projection {} emitted {} hit(s)",
            input.output_projection, input.returned_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input() -> QueryCostPlanInput {
        QueryCostPlanInput {
            query_chars: 12,
            requested_mode: "hybrid".to_string(),
            realized_mode: "hybrid".to_string(),
            fallback_tier: None,
            fallback_reason: None,
            semantic_refinement: true,
            wildcard_fallback: false,
            limit: 10,
            offset: 0,
            returned_count: 10,
            total_matches: 25,
            max_tokens: None,
            tokens_estimated: Some(400),
            hits_clamped: false,
            timeout_ms: None,
            timed_out: false,
            input_cursor_present: false,
            next_cursor_present: true,
            output_projection: "all".to_string(),
            cache_hits: 0,
            cache_misses: 1,
            cache_shortfall: 0,
            aggregation_count: 0,
        }
    }

    #[test]
    fn no_limit_token_budget_reports_token_exhaustion() {
        let plan = build_query_cost_plan(QueryCostPlanInput {
            limit: 0,
            max_tokens: Some(200),
            tokens_estimated: Some(450),
            hits_clamped: true,
            output_projection: "summary".to_string(),
            ..base_input()
        });

        assert_eq!(
            plan.budget_exhaustion.as_ref().map(|b| b.kind),
            Some(BudgetExhaustionKind::TokenBudget)
        );
        assert!(
            plan.phases
                .iter()
                .any(|phase| phase.phase == QueryPhase::Output && phase.reason.contains("clamped"))
        );
    }

    #[test]
    fn huge_snippet_projection_keeps_budget_reason_explicit() {
        let plan = build_query_cost_plan(QueryCostPlanInput {
            max_tokens: Some(100),
            tokens_estimated: Some(2_000),
            hits_clamped: true,
            output_projection: "custom".to_string(),
            ..base_input()
        });

        assert!(
            plan.budget_exhaustion
                .as_ref()
                .expect("budget exhaustion")
                .reason
                .contains("max_tokens")
        );
    }

    #[test]
    fn semantic_unavailable_records_planned_but_unrealized_semantic_phase() {
        let plan = build_query_cost_plan(QueryCostPlanInput {
            realized_mode: "lexical".to_string(),
            semantic_refinement: false,
            fallback_tier: Some("lexical".to_string()),
            fallback_reason: Some("semantic assets unavailable".to_string()),
            ..base_input()
        });

        let semantic = plan
            .phases
            .iter()
            .find(|phase| phase.phase == QueryPhase::Semantic)
            .expect("semantic phase");
        assert!(semantic.planned);
        assert!(!semantic.realized);
        assert!(semantic.reason.contains("semantic assets unavailable"));
    }

    #[test]
    fn cache_hit_and_miss_stats_stay_truthful() {
        let plan = build_query_cost_plan(QueryCostPlanInput {
            cache_hits: 3,
            cache_misses: 2,
            cache_shortfall: 1,
            ..base_input()
        });

        assert!(plan.cache.eligible);
        assert_eq!(plan.cache.hits, 3);
        assert_eq!(plan.cache.misses, 2);
        assert_eq!(plan.cache.shortfall, 1);
    }

    #[test]
    fn cursor_continuation_preserves_identity_window() {
        let plan = build_query_cost_plan(QueryCostPlanInput {
            input_cursor_present: true,
            next_cursor_present: true,
            offset: 10,
            limit: 10,
            returned_count: 10,
            total_matches: 31,
            ..base_input()
        });

        assert!(plan.result_identity.cursor_continuation);
        assert_eq!(
            plan.result_identity.continuity_key,
            "offset:10:limit:10:returned:10:total:31"
        );
        assert_eq!(
            plan.budget_exhaustion.as_ref().map(|b| b.kind),
            Some(BudgetExhaustionKind::CursorPage)
        );
    }

    #[test]
    fn empty_offset_page_still_realizes_output_phase() {
        let plan = build_query_cost_plan(QueryCostPlanInput {
            offset: 100,
            returned_count: 0,
            total_matches: 12,
            next_cursor_present: false,
            ..base_input()
        });

        let output = plan
            .phases
            .iter()
            .find(|phase| phase.phase == QueryPhase::Output)
            .expect("output phase");
        assert!(output.realized);
        assert_eq!(
            plan.result_identity.continuity_key,
            "offset:100:limit:10:returned:0:total:12"
        );
    }
}
