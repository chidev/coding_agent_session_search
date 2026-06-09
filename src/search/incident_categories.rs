// Dead-code tolerated module-wide: this incident-mining category schema
// lands ahead of the bounded discovery (.10.2) and privacy-audit (.10.5)
// passes that classify mined candidates, and the robot-docs update (.11.3).
#![allow(dead_code)]

//! Incident-mining category schema (bead
//! cass-fleet-resilience-20260608-uojcg.10.1).
//!
//! `cass` incident/history triage needs a stable category vocabulary so a
//! mined candidate can be classified, attributed to a root-cause family, and
//! handled under the right privacy tier. This module freezes the report's
//! fourth-pass classifier as that vocabulary: a fixed set of categories, each
//! with a stable id, description, detection signals, example terms, a
//! baseline detection confidence, the associated
//! [`RootCauseFamily`](crate::root_cause_taxonomy::RootCauseFamily), a privacy
//! tier, and a recommended next probe.
//!
//! Forward-compatibility: an explicit [`IncidentCategory::Other`] variant and
//! `from_id` returning `None` for unrecognised ids mean new categories can be
//! added without breaking consumers that match the stable ids. All enums
//! serialize as snake_case.

use serde::{Deserialize, Serialize};

use crate::root_cause_taxonomy::RootCauseFamily;

/// How reliably a category's detection signals indicate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DetectionConfidence {
    Low,
    Medium,
    High,
}

/// Privacy tier governing how a mined incident of this category may be
/// surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrivacyTier {
    /// Operational signals only (no user content or identifying paths).
    Operational,
    /// May reference paths/workspaces/hosts; redact before surfacing.
    Redacted,
    /// May include session content; gated behind explicit consent.
    Sensitive,
}

/// The stable incident categories from the report's fourth-pass classifier.
/// `Other` is the explicit extensibility escape hatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IncidentCategory {
    CassStatusHealth,
    IndexStaleMissing,
    IndexStallProgress,
    SearchZeroWorkspace,
    QuarantineOom,
    StorageBusyCorrupt,
    RemoteSyncAuth,
    Semantic,
    WatchSalvageIssues,
    HostPressure,
    DependencyAttribution,
    /// A future / unclassified category. Never breaks the contract.
    Other,
}

/// The schema for one incident category.
// `&'static [&'static str]` fields can be serialized but not deserialized, so this
// static catalog descriptor is Serialize-only (matches RootCauseDescriptor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct IncidentCategoryDef {
    pub category: IncidentCategory,
    /// Stable snake_case id (matches the serialized `category`).
    pub id: &'static str,
    pub description: &'static str,
    /// Signals a miner looks for (log markers, err.kind, status fields).
    pub detection_signals: &'static [&'static str],
    /// Example query terms that surface this category in history.
    pub example_terms: &'static [&'static str],
    /// Baseline confidence that the signals indicate this category.
    pub confidence: DetectionConfidence,
    /// The root-cause family most often implicated (best-effort a-priori).
    pub root_cause_family: RootCauseFamily,
    pub privacy_tier: PrivacyTier,
    /// The recommended next bounded probe when this category is suspected.
    pub recommended_next_probe: &'static str,
}

impl IncidentCategory {
    /// Stable snake_case id for this category.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::CassStatusHealth => "cass_status_health",
            Self::IndexStaleMissing => "index_stale_missing",
            Self::IndexStallProgress => "index_stall_progress",
            Self::SearchZeroWorkspace => "search_zero_workspace",
            Self::QuarantineOom => "quarantine_oom",
            Self::StorageBusyCorrupt => "storage_busy_corrupt",
            Self::RemoteSyncAuth => "remote_sync_auth",
            Self::Semantic => "semantic",
            Self::WatchSalvageIssues => "watch_salvage_issues",
            Self::HostPressure => "host_pressure",
            Self::DependencyAttribution => "dependency_attribution",
            Self::Other => "other",
        }
    }

    /// Resolve a category from its stable id. Returns `None` for unknown ids
    /// so a future category surfaces as unrecognised rather than silently
    /// mapping to an existing one.
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        CATEGORIES
            .iter()
            .map(|d| d.category)
            .chain(std::iter::once(Self::Other))
            .find(|c| c.id() == id)
    }
}

/// The frozen category schema, in a stable order. `Other` is intentionally
/// excluded from the seeded set (it is the open extension point).
static CATEGORIES: &[IncidentCategoryDef] = &[
    IncidentCategoryDef {
        category: IncidentCategory::CassStatusHealth,
        id: "cass_status_health",
        description: "cass status/health reports degraded, unhealthy, or contradictory readiness",
        detection_signals: &[
            "health_class",
            "recommended_action",
            "unhealthy",
            "degraded",
        ],
        example_terms: &["health", "status", "unhealthy", "recommended action"],
        confidence: DetectionConfidence::High,
        root_cause_family: RootCauseFamily::CassDerivedState,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass health --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::IndexStaleMissing,
        id: "index_stale_missing",
        description: "the lexical index is stale, missing, or not initialized",
        detection_signals: &["index_freshness", "not_initialized", "stale", "OpenRead"],
        example_terms: &["stale index", "no index", "reindex", "index missing"],
        confidence: DetectionConfidence::High,
        root_cause_family: RootCauseFamily::CassDerivedState,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass status --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::IndexStallProgress,
        id: "index_stall_progress",
        description: "an index/rebuild emits heartbeats but makes no forward progress",
        detection_signals: &[
            "stalled",
            "no forward progress",
            "last_progress_at_ms",
            "rebuild",
        ],
        example_terms: &["stalled", "stuck index", "no progress", "rebuild hang"],
        confidence: DetectionConfidence::Medium,
        root_cause_family: RootCauseFamily::CassDerivedState,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass status --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::SearchZeroWorkspace,
        id: "search_zero_workspace",
        description: "a workspace-filtered search returns zero hits due to a path/workspace mismatch",
        detection_signals: &[
            "zero_result_diagnosis",
            "candidate_workspaces",
            "workspace mismatch",
        ],
        example_terms: &[
            "no results",
            "empty workspace",
            "wrong workspace",
            "moved checkout",
        ],
        confidence: DetectionConfidence::Medium,
        root_cause_family: RootCauseFamily::WorkspaceProvenance,
        privacy_tier: PrivacyTier::Redacted,
        recommended_next_probe: "cass sources list --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::QuarantineOom,
        id: "quarantine_oom",
        description: "ingest hit an irreducible streaming OOM and quarantined a conversation",
        detection_signals: &[
            "quarantined_conversations",
            "index-ingest-out-of-memory",
            "ingest_oom",
        ],
        example_terms: &["quarantine", "out of memory", "oom", "poison session"],
        confidence: DetectionConfidence::High,
        root_cause_family: RootCauseFamily::CassDerivedState,
        privacy_tier: PrivacyTier::Redacted,
        recommended_next_probe: "cass diag --json --quarantine",
    },
    IncidentCategoryDef {
        category: IncidentCategory::StorageBusyCorrupt,
        id: "storage_busy_corrupt",
        description: "the storage engine reports busy locks, integrity failures, or WAL sidecar issues",
        detection_signals: &["database is locked", "integrity", "OpenRead", "WAL", "busy"],
        example_terms: &["db locked", "corrupt", "integrity check", "busy timeout"],
        confidence: DetectionConfidence::High,
        root_cause_family: RootCauseFamily::FrankensqliteStorage,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass doctor --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::RemoteSyncAuth,
        id: "remote_sync_auth",
        description: "remote source sync failed on transport or authentication",
        detection_signals: &[
            "ssh",
            "rsync",
            "permission denied",
            "host key",
            "auth",
            "timeout",
        ],
        example_terms: &[
            "sync failed",
            "ssh error",
            "auth",
            "permission denied",
            "host unreachable",
        ],
        confidence: DetectionConfidence::High,
        root_cause_family: RootCauseFamily::RemoteTransportAuth,
        privacy_tier: PrivacyTier::Redacted,
        recommended_next_probe: "cass sources list --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::Semantic,
        id: "semantic",
        description: "semantic search is unavailable, backfilling, or has stale/missing model or vector assets",
        detection_signals: &[
            "semantic_fallback_lexical",
            "fallback_mode",
            "model",
            "vector",
            "embedder",
        ],
        example_terms: &[
            "semantic unavailable",
            "model missing",
            "backfill",
            "hybrid fallback",
        ],
        confidence: DetectionConfidence::High,
        root_cause_family: RootCauseFamily::SemanticAssets,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass status --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::WatchSalvageIssues,
        id: "watch_salvage_issues",
        description: "watch-mode exits, OOM-kill restart loops, or historical salvage re-scans",
        detection_signals: &[
            "--watch",
            "exit code 9",
            "drop_close",
            "salvage",
            "deferred_authoritative_db_rebuild",
        ],
        example_terms: &["watch crash", "exit 9", "salvage loop", "watch restart"],
        confidence: DetectionConfidence::Medium,
        root_cause_family: RootCauseFamily::CassDerivedState,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass status --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::HostPressure,
        id: "host_pressure",
        description: "host memory/load/disk pressure (OOM kills, high load, low free space) drives the incident",
        detection_signals: &["oomd", "load average", "no space left", "swap", "ballast"],
        example_terms: &["out of disk", "oom killed", "high load", "swap thrash"],
        confidence: DetectionConfidence::Medium,
        root_cause_family: RootCauseFamily::HostOomLoad,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass doctor --json",
    },
    IncidentCategoryDef {
        category: IncidentCategory::DependencyAttribution,
        id: "dependency_attribution",
        description: "the incident is plausibly attributable to a pinned sibling dependency vs an upstream fix",
        detection_signals: &[
            "pin_state",
            "upstream_fix_possibly_missing",
            "known_issue_ids",
            "frankensqlite",
            "frankensearch",
        ],
        example_terms: &[
            "dependency",
            "pinned rev",
            "upstream fix",
            "regression after bump",
        ],
        // Attribution is the task; no single family a-priori, so Unknown is the
        // honest seed until discovery narrows it.
        confidence: DetectionConfidence::Low,
        root_cause_family: RootCauseFamily::Unknown,
        privacy_tier: PrivacyTier::Operational,
        recommended_next_probe: "cass diag --json",
    },
];

/// The frozen incident category schema, in stable order (excludes `Other`).
pub(crate) fn categories() -> &'static [IncidentCategoryDef] {
    CATEGORIES
}

/// Look up a category definition by stable id.
pub(crate) fn category_def(id: &str) -> Option<&'static IncidentCategoryDef> {
    CATEGORIES.iter().find(|d| d.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eleven required category ids from the report, in order.
    const REQUIRED: &[&str] = &[
        "cass_status_health",
        "index_stale_missing",
        "index_stall_progress",
        "search_zero_workspace",
        "quarantine_oom",
        "storage_busy_corrupt",
        "remote_sync_auth",
        "semantic",
        "watch_salvage_issues",
        "host_pressure",
        "dependency_attribution",
    ];

    #[test]
    fn schema_lists_the_required_categories_in_stable_order() {
        let ids: Vec<&str> = categories().iter().map(|d| d.id).collect();
        assert_eq!(ids, REQUIRED, "category set and order must be stable");
    }

    #[test]
    fn every_def_id_matches_its_serialized_category() {
        for d in categories() {
            // The struct `id` and the enum's snake_case serialization agree.
            assert_eq!(d.id, d.category.id(), "id mismatch for {:?}", d.category);
            let json = serde_json::to_string(&d.category).unwrap();
            assert_eq!(json, format!("\"{}\"", d.id));
        }
    }

    #[test]
    fn every_def_has_signals_terms_and_a_probe() {
        for d in categories() {
            assert!(!d.detection_signals.is_empty(), "{} needs signals", d.id);
            assert!(!d.example_terms.is_empty(), "{} needs terms", d.id);
            assert!(!d.description.is_empty(), "{} needs a description", d.id);
            assert!(
                !d.recommended_next_probe.is_empty(),
                "{} needs a next probe",
                d.id
            );
            // Probes are concrete cass commands, never a bare `cass`.
            assert!(
                d.recommended_next_probe.starts_with("cass "),
                "{} probe should be a concrete cass command: {}",
                d.id,
                d.recommended_next_probe
            );
        }
    }

    #[test]
    fn from_id_resolves_known_categories_and_other() {
        for id in REQUIRED {
            assert_eq!(IncidentCategory::from_id(id).map(|c| c.id()), Some(*id));
        }
        assert_eq!(
            IncidentCategory::from_id("other"),
            Some(IncidentCategory::Other)
        );
    }

    #[test]
    fn unknown_category_id_is_unrecognised_not_silently_mapped() {
        // Forward-compat: a future/unknown id must NOT resolve to an existing
        // category; consumers can then treat it as Other explicitly.
        assert_eq!(IncidentCategory::from_id("brand_new_category_v2"), None);
        assert!(category_def("brand_new_category_v2").is_none());
    }

    #[test]
    fn root_cause_families_are_assigned_consistently() {
        // Spot-check the contract-critical associations.
        assert_eq!(
            category_def("storage_busy_corrupt")
                .unwrap()
                .root_cause_family,
            RootCauseFamily::FrankensqliteStorage
        );
        assert_eq!(
            category_def("remote_sync_auth").unwrap().root_cause_family,
            RootCauseFamily::RemoteTransportAuth
        );
        assert_eq!(
            category_def("semantic").unwrap().root_cause_family,
            RootCauseFamily::SemanticAssets
        );
        assert_eq!(
            category_def("search_zero_workspace")
                .unwrap()
                .root_cause_family,
            RootCauseFamily::WorkspaceProvenance
        );
        // Attribution category seeds Unknown by design.
        assert_eq!(
            category_def("dependency_attribution")
                .unwrap()
                .root_cause_family,
            RootCauseFamily::Unknown
        );
    }

    #[test]
    fn privacy_tiers_redact_path_and_host_bearing_categories() {
        assert_eq!(
            category_def("search_zero_workspace").unwrap().privacy_tier,
            PrivacyTier::Redacted
        );
        assert_eq!(
            category_def("remote_sync_auth").unwrap().privacy_tier,
            PrivacyTier::Redacted
        );
        assert_eq!(
            category_def("quarantine_oom").unwrap().privacy_tier,
            PrivacyTier::Redacted
        );
    }

    #[test]
    fn category_def_serializes_with_stable_field_wire_forms() {
        // The schema is serialize-only (static borrowed fields); assert the
        // projected JSON shape rather than a deserialize round-trip.
        let d = category_def("semantic").unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(d).unwrap()).unwrap();
        assert_eq!(v["category"], "semantic");
        assert_eq!(v["root_cause_family"], "semantic-assets");
        assert_eq!(v["privacy_tier"], "operational");
        assert_eq!(v["confidence"], "high");
        assert_eq!(v["id"], "semantic");
        assert!(v["detection_signals"].is_array());
        assert!(
            v["recommended_next_probe"]
                .as_str()
                .unwrap()
                .starts_with("cass ")
        );
    }
}
