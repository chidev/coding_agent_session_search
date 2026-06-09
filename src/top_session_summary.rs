//! Bounded top-session summaries for recurrent problem clusters.
//!
//! Bead: coding_agent_session_search-cass-fleet-resilience-20260608-uojcg.10.3
//! ("Preserve top-session pointers and category breadth summaries").
//!
//! Incident mining over a large corpus can surface tens of thousands of raw
//! matches across many categories. Dumping those is useless and unsafe. This
//! module is the pure **summarizer**: it collapses a stream of categorized hits
//! into a bounded, ranked list of the top sessions/files carrying the most (and
//! the broadest) problem clusters, each with category breadth, dominant
//! categories, host/path identity, archive-only state, redaction status, and a
//! single safe `cass view`/`cass pack` pointer — instead of the raw matches.
//!
//! Pure and offline: the caller (incident mining, bead `10.2`) feeds already
//! bounded, redacted hits; this module only aggregates and ranks. The suggested
//! command is always a safe, read-only `--json` pointer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Stable schema version for the top-session summary wire format.
pub const TOP_SESSION_SCHEMA_VERSION: u32 = 1;

/// Whether the underlying session file still exists, or is known only from the
/// archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionExistsState {
    /// The source file exists on disk.
    Exists,
    /// Known only from the archive; the live source is gone/pruned.
    ArchiveOnly,
    /// Existence could not be determined.
    Unknown,
}

impl SessionExistsState {
    /// Stable snake_case wire label.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionExistsState::Exists => "exists",
            SessionExistsState::ArchiveOnly => "archive_only",
            SessionExistsState::Unknown => "unknown",
        }
    }
}

/// Redaction status of the evidence summary carried for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionStatus {
    /// Evidence was redacted before inclusion.
    Redacted,
    /// No sensitive content; nothing to redact.
    NotApplicable,
}

impl RedactionStatus {
    /// Stable snake_case wire label.
    pub fn as_str(self) -> &'static str {
        match self {
            RedactionStatus::Redacted => "redacted",
            RedactionStatus::NotApplicable => "not_applicable",
        }
    }
}

/// A single categorized incident hit, as fed by the mining layer. `category` is
/// a free label (the canonical category taxonomy lives in the mining layer; this
/// summarizer is decoupled from it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentHit {
    /// Session identifier the hit belongs to.
    pub session_id: String,
    /// Host the session came from.
    pub host: String,
    /// Source path or source_id pointer.
    pub path_or_source_id: String,
    /// Existence/archive state when known.
    pub exists_state: SessionExistsState,
    /// Problem category label.
    pub category: String,
    /// Whether this hit's evidence is redacted.
    pub redacted: bool,
}

/// One ranked top-session entry in the summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopSessionEntry {
    /// Session identifier.
    pub session_id: String,
    /// Source host.
    pub host: String,
    /// Source path or source_id.
    pub path_or_source_id: String,
    /// Existence/archive state.
    pub exists_state: SessionExistsState,
    /// Total hits in this session.
    pub hit_count: usize,
    /// Number of distinct categories (breadth).
    pub category_breadth: usize,
    /// Dominant categories, most-frequent first (ties broken by label), capped.
    pub dominant_categories: Vec<String>,
    /// Redaction status of the carried evidence.
    pub redaction_status: RedactionStatus,
    /// Safe, read-only pointer command for an operator/agent.
    pub suggested_command: String,
}

/// The bounded top-session summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopSessionSummary {
    /// Mirrors [`TOP_SESSION_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Ranked top sessions (bounded by the cap).
    pub top_sessions: Vec<TopSessionEntry>,
    /// Distinct sessions observed (pre-cap).
    pub total_sessions: usize,
    /// Total hits observed.
    pub total_hits: usize,
    /// True when more sessions existed than the cap returned.
    pub truncated: bool,
}

/// How many dominant categories to list per session.
const DOMINANT_CATEGORY_CAP: usize = 5;

struct SessionAccum {
    host: String,
    path_or_source_id: String,
    exists_state: SessionExistsState,
    hit_count: usize,
    any_redacted: bool,
    category_counts: BTreeMap<String, usize>,
}

/// Summarize categorized hits into the top `top_n` sessions by hit count (then
/// category breadth, then stable session id). Pure; no I/O. `top_n` of 0 yields
/// an empty list but still reports totals and `truncated`.
pub fn summarize_top_sessions(hits: &[IncidentHit], top_n: usize) -> TopSessionSummary {
    let mut sessions: BTreeMap<String, SessionAccum> = BTreeMap::new();
    let total_hits = hits.len();

    for hit in hits {
        let entry = sessions
            .entry(hit.session_id.clone())
            .or_insert_with(|| SessionAccum {
                host: hit.host.clone(),
                path_or_source_id: hit.path_or_source_id.clone(),
                exists_state: hit.exists_state,
                hit_count: 0,
                any_redacted: false,
                category_counts: BTreeMap::new(),
            });
        entry.hit_count += 1;
        entry.any_redacted |= hit.redacted;
        *entry.category_counts.entry(hit.category.clone()).or_insert(0) += 1;
        // Prefer a definite existence state over Unknown if any hit knows it.
        if entry.exists_state == SessionExistsState::Unknown
            && hit.exists_state != SessionExistsState::Unknown
        {
            entry.exists_state = hit.exists_state;
        }
    }

    let total_sessions = sessions.len();

    let mut ranked: Vec<(String, SessionAccum)> = sessions.into_iter().collect();
    // Rank: hit_count desc, then breadth desc, then session_id asc (stable).
    ranked.sort_by(|(a_id, a), (b_id, b)| {
        b.hit_count
            .cmp(&a.hit_count)
            .then_with(|| b.category_counts.len().cmp(&a.category_counts.len()))
            .then_with(|| a_id.cmp(b_id))
    });

    let truncated = top_n < ranked.len();
    let top_sessions: Vec<TopSessionEntry> = ranked
        .into_iter()
        .take(top_n)
        .map(|(session_id, acc)| {
            // Dominant categories: count desc, then label asc; capped.
            let mut cats: Vec<(String, usize)> = acc.category_counts.iter().map(|(k, v)| (k.clone(), *v)).collect();
            cats.sort_by(|(a_k, a_v), (b_k, b_v)| b_v.cmp(a_v).then_with(|| a_k.cmp(b_k)));
            let dominant_categories: Vec<String> = cats
                .into_iter()
                .take(DOMINANT_CATEGORY_CAP)
                .map(|(k, _)| k)
                .collect();
            let redaction_status = if acc.any_redacted {
                RedactionStatus::Redacted
            } else {
                RedactionStatus::NotApplicable
            };
            let suggested_command = format!("cass view {session_id} --json");
            TopSessionEntry {
                session_id,
                host: acc.host,
                path_or_source_id: acc.path_or_source_id,
                exists_state: acc.exists_state,
                hit_count: acc.hit_count,
                category_breadth: acc.category_counts.len(),
                dominant_categories,
                redaction_status,
                suggested_command,
            }
        })
        .collect();

    TopSessionSummary {
        schema_version: TOP_SESSION_SCHEMA_VERSION,
        top_sessions,
        total_sessions,
        total_hits,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(session: &str, host: &str, category: &str) -> IncidentHit {
        IncidentHit {
            session_id: session.to_string(),
            host: host.to_string(),
            path_or_source_id: format!("{host}:/sessions/{session}.jsonl"),
            exists_state: SessionExistsState::Exists,
            category: category.to_string(),
            redacted: false,
        }
    }

    #[test]
    fn aggregates_hits_and_category_breadth_per_session() {
        let hits = vec![
            hit("s1", "local", "timeout"),
            hit("s1", "local", "storage"),
            hit("s1", "local", "timeout"),
            hit("s2", "ts1", "auth"),
        ];
        let summary = summarize_top_sessions(&hits, 10);
        assert_eq!(summary.total_sessions, 2);
        assert_eq!(summary.total_hits, 4);
        assert!(!summary.truncated);
        let s1 = &summary.top_sessions[0];
        assert_eq!(s1.session_id, "s1");
        assert_eq!(s1.hit_count, 3);
        assert_eq!(s1.category_breadth, 2); // timeout, storage
        assert_eq!(s1.dominant_categories[0], "timeout"); // most frequent
    }

    #[test]
    fn ranks_by_hit_count_then_breadth() {
        let mut hits = Vec::new();
        // busy: 5 hits, 1 category.
        for _ in 0..5 {
            hits.push(hit("busy", "local", "timeout"));
        }
        // broad: 5 hits, 3 categories (more breadth, same hit count).
        hits.push(hit("broad", "ts1", "a"));
        hits.push(hit("broad", "ts1", "b"));
        hits.push(hit("broad", "ts1", "c"));
        hits.push(hit("broad", "ts1", "a"));
        hits.push(hit("broad", "ts1", "b"));
        let summary = summarize_top_sessions(&hits, 10);
        // Equal hit_count (5), broader session ranks first.
        assert_eq!(summary.top_sessions[0].session_id, "broad");
        assert_eq!(summary.top_sessions[0].category_breadth, 3);
    }

    #[test]
    fn caps_to_top_n_and_marks_truncated() {
        let hits: Vec<IncidentHit> = (0..20)
            .flat_map(|i| {
                let n = 20 - i; // session i gets 20-i hits => i ordering by count
                (0..n).map(move |_| hit(&format!("s{i:02}"), "local", "timeout"))
            })
            .collect();
        let summary = summarize_top_sessions(&hits, 3);
        assert_eq!(summary.top_sessions.len(), 3);
        assert_eq!(summary.total_sessions, 20);
        assert!(summary.truncated);
        // Top session has the most hits (s00 with 20).
        assert_eq!(summary.top_sessions[0].session_id, "s00");
    }

    #[test]
    fn archive_only_state_and_redaction_are_preserved() {
        let mut h1 = hit("gone", "mac-mini-max", "storage");
        h1.exists_state = SessionExistsState::ArchiveOnly;
        h1.redacted = true;
        let summary = summarize_top_sessions(&[h1], 10);
        let s = &summary.top_sessions[0];
        assert_eq!(s.exists_state, SessionExistsState::ArchiveOnly);
        assert_eq!(s.redaction_status, RedactionStatus::Redacted);
    }

    #[test]
    fn definite_exists_state_wins_over_unknown() {
        let mut unknown = hit("s", "local", "a");
        unknown.exists_state = SessionExistsState::Unknown;
        let mut known = hit("s", "local", "b");
        known.exists_state = SessionExistsState::ArchiveOnly;
        let summary = summarize_top_sessions(&[unknown, known], 10);
        assert_eq!(summary.top_sessions[0].exists_state, SessionExistsState::ArchiveOnly);
    }

    #[test]
    fn dominant_categories_are_capped_and_ordered() {
        let mut hits = Vec::new();
        for (cat, n) in [("a", 6), ("b", 5), ("c", 4), ("d", 3), ("e", 2), ("f", 1)] {
            for _ in 0..n {
                hits.push(hit("s", "local", cat));
            }
        }
        let summary = summarize_top_sessions(&hits, 10);
        let s = &summary.top_sessions[0];
        assert_eq!(s.category_breadth, 6);
        assert_eq!(s.dominant_categories.len(), DOMINANT_CATEGORY_CAP); // capped at 5
        assert_eq!(s.dominant_categories, vec!["a", "b", "c", "d", "e"]); // by frequency
    }

    #[test]
    fn suggested_command_is_safe_and_read_only() {
        let summary = summarize_top_sessions(&[hit("s1", "local", "x")], 10);
        let cmd = summary.top_sessions[0].suggested_command.to_ascii_lowercase();
        assert!(cmd.starts_with("cass view ") || cmd.starts_with("cass pack "));
        assert!(cmd.contains("--json"));
        for needle in ["--delete", "rm -rf", "prune", "index", "repair"] {
            assert!(!cmd.contains(needle), "unsafe suggested command: {cmd:?}");
        }
    }

    #[test]
    fn empty_input_and_zero_cap_are_well_defined() {
        let empty = summarize_top_sessions(&[], 10);
        assert_eq!(empty.total_sessions, 0);
        assert!(empty.top_sessions.is_empty());
        assert!(!empty.truncated);

        let zero_cap = summarize_top_sessions(&[hit("s", "h", "c")], 0);
        assert!(zero_cap.top_sessions.is_empty());
        assert_eq!(zero_cap.total_sessions, 1);
        assert!(zero_cap.truncated);
    }

    #[test]
    fn json_contract_is_stable_and_round_trips() {
        let summary = summarize_top_sessions(&[hit("s1", "local", "timeout")], 10);
        let value = serde_json::to_value(&summary).unwrap();
        assert_eq!(value["schema_version"], TOP_SESSION_SCHEMA_VERSION);
        assert_eq!(value["top_sessions"][0]["exists_state"], "exists");
        assert_eq!(value["top_sessions"][0]["redaction_status"], "not_applicable");
        let back: TopSessionSummary = serde_json::from_value(value).unwrap();
        assert_eq!(back, summary);
    }
}
