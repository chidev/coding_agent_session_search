//! Deterministic fixture and golden checks for the planned `cass swarm status`
//! robot contract.
//!
//! These tests intentionally do not run live Agent Mail, git remotes, rch jobs,
//! cargo, cass indexing, or private session-log reads. They pin the fixture
//! surface that implementation beads can consume once the command exists.
//!
//! ## Regenerate
//!
//! ```bash
//! UPDATE_GOLDENS=1 rch exec -- env CARGO_TARGET_DIR=/tmp/cass-swarm-status-golden-target cargo test --test swarm_status_contract
//! git diff -- tests/fixtures/swarm_status tests/golden/swarm_status tests/swarm_status_contract.rs
//! ```

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_ROOT: &str = "tests/fixtures/swarm_status";
const MANIFEST_PATH: &str = "tests/fixtures/swarm_status/manifest.json";
const GOLDEN_UPDATE_COMMAND_SHAPE: &str = "UPDATE_GOLDENS=1 rch exec -- env CARGO_TARGET_DIR=/tmp/cass-swarm-status-golden-target cargo test --test swarm_status_contract";
const GOLDEN_REVIEW_COMMAND_SHAPE: &str = "git diff -- tests/fixtures/swarm_status tests/golden/swarm_status tests/swarm_status_contract.rs";

const REQUIRED_SCENARIOS: &[&str] = &[
    "healthy",
    "busy",
    "stale_advisory",
    "reservation_conflict",
    "build_pressure",
    "no_ready_work",
    "privacy_guardrails",
];

const REQUIRED_TOP_LEVEL_KEYS: &[&str] = &[
    "_meta",
    "agents",
    "beads",
    "build_pressure",
    "cass",
    "evidence",
    "git",
    "privacy",
    "providers",
    "recommendations",
    "reservations",
    "schema_version",
    "status",
    "summary",
];

const REQUIRED_PROVIDER_NAMES: &[&str] = &[
    "agent_mail",
    "beads",
    "cass_health",
    "cass_status",
    "evidence",
    "git",
    "process",
];

#[test]
fn swarm_status_manifest_hashes_are_current() {
    let manifest = read_json(repo_path(MANIFEST_PATH));
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["contract"], "cass.swarm.status.v1");

    for scenario in scenarios(&manifest) {
        let fixture_id = string_field(scenario, "fixture_id");
        let input_path = repo_path(string_field(scenario, "input_path"));
        let golden_path = repo_path(string_field(scenario, "golden_path"));

        assert_eq!(
            sha256_hex(&input_path),
            string_field(scenario, "input_sha256"),
            "{fixture_id} input hash drifted"
        );
        assert_eq!(
            sha256_hex(&golden_path),
            string_field(scenario, "golden_sha256"),
            "{fixture_id} golden hash drifted"
        );

        assert_eq!(
            string_field(scenario, "command_shape"),
            format!("cass swarm status --json --fixture-dir {FIXTURE_ROOT}"),
            "{fixture_id} command shape should stay robot-safe and fixture-backed"
        );
        assert_eq!(
            string_field(scenario, "stdout_capture_path"),
            string_field(scenario, "golden_path"),
            "{fixture_id} stdout capture should be the reviewed golden"
        );
        assert_eq!(string_field(scenario, "stderr_capture"), "");
        assert!(
            !string_field(scenario, "assertion_summary").is_empty(),
            "{fixture_id} missing assertion summary"
        );

        let redaction = scenario
            .get("redaction_report")
            .and_then(Value::as_object)
            .unwrap_or_else(|| panic!("{fixture_id} missing redaction_report"));
        assert_eq!(
            redaction.get("raw_session_content_included"),
            Some(&Value::Bool(false)),
            "{fixture_id} fixtures must not include raw session content"
        );
        assert_eq!(
            redaction.get("mail_body_snippets_included"),
            Some(&Value::Bool(false)),
            "{fixture_id} base fixtures must stay metadata-only for mail"
        );
    }
}

#[test]
fn swarm_status_golden_update_workflow_is_pinned() {
    let manifest = read_json(repo_path(MANIFEST_PATH));
    let update_workflow = manifest
        .get("golden_update_workflow")
        .and_then(Value::as_object)
        .expect("manifest golden_update_workflow must be an object");

    assert_eq!(
        update_workflow.get("command_shape").and_then(Value::as_str),
        Some(GOLDEN_UPDATE_COMMAND_SHAPE),
        "golden update workflow must require UPDATE_GOLDENS=1 and rch"
    );
    assert_eq!(
        update_workflow
            .get("review_command")
            .and_then(Value::as_str),
        Some(GOLDEN_REVIEW_COMMAND_SHAPE),
        "golden update workflow must require explicit diff review"
    );
    assert_eq!(
        update_workflow.get("review_required"),
        Some(&Value::Bool(true)),
        "golden updates require human review before commit"
    );
    assert_eq!(
        update_workflow.get("uses_live_services"),
        Some(&Value::Bool(false)),
        "golden updates must stay fixture-only"
    );
}

#[test]
fn swarm_status_fixture_set_covers_required_scenarios() {
    let manifest = read_json(repo_path(MANIFEST_PATH));
    let actual: BTreeSet<&str> = scenarios(&manifest)
        .iter()
        .map(|scenario| string_field(scenario, "fixture_id"))
        .collect();
    let expected: BTreeSet<&str> = REQUIRED_SCENARIOS.iter().copied().collect();
    assert_eq!(actual, expected);
}

#[test]
fn swarm_status_goldens_follow_contract_shape() {
    let manifest = read_json(repo_path(MANIFEST_PATH));
    for scenario in scenarios(&manifest) {
        let fixture_id = string_field(scenario, "fixture_id");
        let input = read_json(repo_path(string_field(scenario, "input_path")));
        let output = read_json(repo_path(string_field(scenario, "golden_path")));

        assert_eq!(input["fixture_id"], fixture_id);
        assert_eq!(output["schema_version"], "cass.swarm.status.v1");
        assert!(
            matches!(output["status"].as_str(), Some("ok" | "partial")),
            "{fixture_id} status must be ok or partial"
        );

        for key in REQUIRED_TOP_LEVEL_KEYS {
            assert!(
                output.get(key).is_some(),
                "{fixture_id} missing top-level key {key}"
            );
        }

        let provider_names: BTreeSet<&str> = output["providers"]
            .as_array()
            .unwrap_or_else(|| panic!("{fixture_id} providers must be an array"))
            .iter()
            .map(|provider| string_field(provider, "name"))
            .collect();
        for provider in REQUIRED_PROVIDER_NAMES {
            assert!(
                provider_names.contains(provider),
                "{fixture_id} missing provider {provider}"
            );
        }
        assert!(
            provider_names.contains("evidence"),
            "{fixture_id} exposes top-level evidence without evidence provider status"
        );

        assert_eq!(
            output["privacy"]["raw_session_content_included"],
            Value::Bool(false),
            "{fixture_id} must not include raw session content"
        );
        assert_eq!(
            output["privacy"]["redaction_policy"], "strict",
            "{fixture_id} must default to strict redaction"
        );
        assert!(
            output["recommendations"]
                .as_array()
                .is_some_and(|items| !items.is_empty()),
            "{fixture_id} should include at least one branchable recommendation"
        );

        assert_no_forbidden_fixture_leaks(fixture_id, &output);
    }
}

#[test]
fn swarm_status_scenario_invariants_are_pinned() {
    let manifest = read_json(repo_path(MANIFEST_PATH));
    for scenario in scenarios(&manifest) {
        let fixture_id = string_field(scenario, "fixture_id");
        let output = read_json(repo_path(string_field(scenario, "golden_path")));

        match fixture_id {
            "healthy" => {
                assert_eq!(output["summary"]["ready_count"], 1);
                assert_eq!(output["summary"]["build_pressure"], "none");
                assert_eq!(output["recommendations"][0]["kind"], "claim-ready-bead");
            }
            "busy" => {
                assert_eq!(output["summary"]["active_agent_count"], 2);
                assert_eq!(output["summary"]["active_reservation_count"], 1);
                assert_eq!(output["summary"]["dirty_worktree"], true);
                assert_eq!(output["recommendations"][0]["kind"], "coordinate");
            }
            "stale_advisory" => {
                assert_eq!(output["summary"]["stale_candidate_count"], 1);
                assert_eq!(
                    output["beads"]["stale_candidates"][0]["stale_state"],
                    "likely_stale"
                );
                assert_eq!(
                    output["recommendations"][0]["requires_human_confirmation"],
                    true
                );
            }
            "reservation_conflict" => {
                assert_eq!(output["beads"]["ready"][0]["safe_to_claim"], false);
                assert_eq!(output["reservations"][0]["state"], "conflicting");
                assert_eq!(output["recommendations"][0]["kind"], "coordinate");
            }
            "build_pressure" => {
                assert_eq!(output["summary"]["build_pressure"], "high");
                assert_eq!(output["build_pressure"]["active_rch_jobs"], 9);
                assert_eq!(output["build_pressure"]["active_cargo_jobs"], 1);
                assert_eq!(
                    output["recommendations"][0]["kind"],
                    "reduce-build-pressure"
                );
            }
            "no_ready_work" => {
                assert_eq!(output["summary"]["ready_count"], 0);
                assert_eq!(output["summary"]["recommended_action"], "no-ready-work");
                assert_eq!(output["recommendations"][0]["kind"], "no-ready-work");
            }
            "privacy_guardrails" => {
                assert_eq!(output["privacy"]["redaction_applied"], true);
                assert_eq!(output["privacy"]["sensitive_paths_scrubbed"], 4);
                assert_eq!(output["privacy"]["command_arguments_scrubbed"], 2);
                assert_eq!(output["privacy"]["env_values_scrubbed"], 1);
                assert_eq!(output["privacy"]["mailbox_snippets_omitted"], 1);
                assert_eq!(output["privacy"]["evidence_references_scrubbed"], 1);
                assert_eq!(
                    output["privacy"]["opt_in_boundary"],
                    "mail body snippets require --include-evidence; raw session content is unsupported in cass.swarm.status.v1"
                );
                assert_eq!(
                    output["evidence"]["recent_threads"][0]["body_snippet"],
                    "[MAIL_BODY_OMITTED]"
                );
                assert_eq!(
                    output["evidence"]["recent_proofs"][0]["redaction_status"],
                    "redacted"
                );
            }
            other => panic!("unexpected scenario {other}"),
        }
    }
}

fn repo_path(relative: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_json(path: PathBuf) -> Value {
    let body =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_str(&body).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn scenarios(manifest: &Value) -> Vec<&Value> {
    manifest["scenarios"]
        .as_array()
        .expect("manifest scenarios must be an array")
        .iter()
        .collect()
}

fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {field} in {value:#}"))
}

fn sha256_hex(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn assert_no_forbidden_fixture_leaks(fixture_id: &str, value: &Value) {
    let text = serde_json::to_string(value).expect("serialize output");
    for needle in [
        "/home/",
        "BEGIN PRIVATE",
        "PRIVATE KEY",
        "SECRET_VALUE",
        "TOKEN=",
        "raw_session_text",
        "/Users/",
        "alice@example.com",
        "api.example.corp",
        "PRIVATE_SESSION_DO_NOT_LEAK",
    ] {
        assert!(
            !text.contains(needle),
            "{fixture_id} golden leaks forbidden fixture text: {needle}"
        );
    }
}
