mod util;

use std::collections::{BTreeMap, BTreeSet};
use util::doctor_e2e_runner::{
    DoctorE2eArtifactManifest, DoctorE2eCliArgs, DoctorE2eRunner, DoctorE2eScenarioSpec,
    default_doctor_e2e_run_root, default_doctor_e2e_scenarios, doctor_e2e_scenarios_for_args,
    parse_doctor_json_stdout, select_scenarios, validate_artifact_manifest,
    validate_artifact_manifest_value,
};
use util::doctor_fixture::{
    DoctorFixtureFactory, DoctorFixtureScenario, default_expected_artifact_keys,
};

#[test]
fn doctor_e2e_cli_args_parse_labels_scenarios_and_flags() {
    let parsed = DoctorE2eCliArgs::parse_from([
        "doctor_v2",
        "--label",
        "quick,privacy",
        "--scenario",
        "quick-source-pruned",
        "--fail-fast",
        "--include-failure-self-test",
    ])
    .expect("parse doctor e2e args");

    assert_eq!(
        parsed.label_filter,
        BTreeSet::from(["privacy".to_string(), "quick".to_string()])
    );
    assert_eq!(
        parsed.scenario_filter,
        BTreeSet::from(["quick-source-pruned".to_string()])
    );
    assert!(parsed.fail_fast);
    assert!(parsed.include_failure_self_test);
}

#[test]
fn doctor_e2e_label_filter_selects_matching_scenarios() {
    let scenarios = default_doctor_e2e_scenarios();
    let parsed = DoctorE2eCliArgs::parse_from(["doctor_v2", "--label", "fault"])
        .expect("parse label filter");
    let selected = select_scenarios(&parsed, &scenarios);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].scenario_id, "quick-mirror-missing");
}

#[test]
fn doctor_e2e_include_failure_self_test_selects_intentional_failure() {
    let parsed = DoctorE2eCliArgs::parse_from([
        "doctor_v2",
        "--label",
        "quick",
        "--include-failure-self-test",
    ])
    .expect("parse self-test flag");
    let scenarios = doctor_e2e_scenarios_for_args(&parsed);
    let selected = select_scenarios(&parsed, &scenarios);

    assert!(
        selected
            .iter()
            .any(|scenario| scenario.scenario_id == "intentional-failure-self-test"),
        "include flag should add and select the failure self-test scenario"
    );
    let self_test = selected
        .iter()
        .find(|scenario| scenario.scenario_id == "intentional-failure-self-test")
        .expect("selected self-test scenario");
    assert_eq!(self_test.expected_runner_status(), "fail");
}

#[test]
fn doctor_fixture_source_truncation_keeps_mirror_and_present_source_distinct() {
    let mut fixture = DoctorFixtureFactory::new("source-truncated-helper");
    fixture.apply_scenario(DoctorFixtureScenario::SourceTruncated);
    fixture
        .validate_manifest()
        .expect("truncated-source fixture manifest should remain internally consistent");

    let manifest = fixture.manifest();
    assert_eq!(
        manifest.expected_coverage_state,
        "source-truncated-mirror-verified"
    );
    assert_eq!(
        manifest
            .expected_source_inventory
            .missing_current_source_count,
        0,
        "fixture should model source truncation without pretending the source file is gone"
    );
    assert_eq!(
        manifest.expected_source_inventory.mirrored_source_count, 1,
        "fixture should keep the pre-truncation raw mirror as recovery evidence"
    );
    assert!(
        manifest
            .expected_anomalies
            .iter()
            .any(|anomaly| anomaly == "upstream-source-truncated")
    );
    assert!(
        manifest.artifacts.iter().any(|artifact| {
            artifact.artifact_kind == "provider_source_truncated"
                && artifact.relative_path.contains(".codex/")
        }),
        "fixture should record the truncated provider source artifact"
    );
    assert!(
        manifest.structured_log.iter().any(|entry| {
            entry.step == "overwrite_file_for_fixture_drift"
                && entry.detail.contains("provider_source_truncated")
        }),
        "fixture should log that upstream bytes drifted after mirror capture"
    );
}

#[test]
fn doctor_e2e_runner_refuses_unsafe_run_roots() {
    let err = DoctorE2eRunner::new("relative/run-root").expect_err("relative root rejected");
    assert!(
        err.contains("must be absolute"),
        "error should explain unsafe root, got: {err}"
    );
}

#[test]
fn doctor_e2e_json_parse_failures_are_diagnostic() {
    let err = parse_doctor_json_stdout(b"not json").expect_err("invalid json rejected");
    assert!(
        err.contains("not valid JSON"),
        "parse failure should be actionable, got: {err}"
    );
}

#[test]
fn doctor_e2e_manifest_validation_rejects_missing_artifacts() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let mut artifacts = BTreeMap::new();
    for key in default_expected_artifact_keys() {
        artifacts.insert(key.to_string(), format!("{key}.missing"));
    }
    let manifest = DoctorE2eArtifactManifest {
        schema_version: 1,
        scenario_id: "missing-artifact".to_string(),
        labels: vec!["quick".to_string()],
        status: "pass".to_string(),
        artifact_dir: "[doctor-e2e-artifacts]".to_string(),
        fixture_root: "[doctor-e2e-fixture]".to_string(),
        home_dir: "[doctor-e2e-home]".to_string(),
        data_dir: "[doctor-e2e-data]".to_string(),
        command_count: 1,
        artifacts,
        failure_context: None,
    };

    let err = validate_artifact_manifest_value(temp.path(), &manifest)
        .expect_err("missing artifact paths rejected");
    assert!(
        err.contains("is missing"),
        "manifest validator should identify absent artifact files, got: {err}"
    );
}

#[test]
fn doctor_e2e_runner_records_artifacts_and_no_mutation_for_pruned_source() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let runner = DoctorE2eRunner::new(temp.path().join("run")).expect("runner");
    let spec = DoctorE2eScenarioSpec::new(
        "artifact-pruned-source",
        DoctorFixtureScenario::SourcePruned,
        ["quick", "source-mirror"],
    )
    .require_json_pointer("/source_inventory")
    .require_json_pointer("/raw_mirror")
    .require_json_pointer("/doctor_command/surface")
    .require_json_pointer("/check_scope/skipped_expensive_collectors")
    .require_json_pointer("/active_repair")
    .require_json_pointer("/operation_outcome/kind")
    .require_json_pointer("/operation_state/mutating_doctor_allowed")
    .require_json_pointer("/source_authority/selected_authority");

    let result = runner.run_scenario(&spec).expect("run doctor e2e scenario");
    assert_eq!(result.status, "pass");
    validate_artifact_manifest(&result.manifest_path).expect("artifact manifest valid");

    for relative in [
        "manifest.json",
        "scenario.json",
        "fixture-inventory.json",
        "source-inventory-before.json",
        "source-inventory-after.json",
        "execution-flow.jsonl",
        "commands.jsonl",
        "stdout/doctor-json.out",
        "stderr/doctor-json.err",
        "parsed-json/doctor-json.json",
        "candidate-staging.json",
        "file-tree-before.json",
        "file-tree-after.json",
        "checksums.json",
        "timing.json",
        "receipts.jsonl",
        "doctor-events.jsonl",
    ] {
        assert!(
            result.artifact_dir.join(relative).exists(),
            "missing expected artifact {relative}"
        );
    }

    let stdout =
        std::fs::read_to_string(result.artifact_dir.join("stdout/doctor-json.out")).unwrap();
    assert!(
        !stdout.contains(temp.path().to_string_lossy().as_ref()),
        "stdout artifact should redact temp paths"
    );
    assert!(
        !stdout.contains("CASS_DOCTOR_PRIVACY_SENTINEL"),
        "stdout artifact should not leak privacy sentinels"
    );

    let doctor_events =
        std::fs::read_to_string(result.artifact_dir.join("doctor-events.jsonl")).unwrap();
    assert!(
        doctor_events.contains("\"phase\":\"operation_started\""),
        "doctor event artifact should preserve the real doctor operation event stream"
    );
    assert!(
        doctor_events.contains("\"hash_chain_tip\"")
            || doctor_events.contains("\"previous_event_hash\""),
        "doctor event artifact should include hash-chain evidence for debugging"
    );

    let fixture_inventory: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("fixture-inventory.json")).unwrap(),
    )
    .expect("fixture inventory json");
    assert_eq!(
        fixture_inventory["scenario_id"].as_str(),
        Some("artifact-pruned-source")
    );
    assert_eq!(
        fixture_inventory["db_row_counts"]["status"].as_str(),
        Some("ok")
    );
    assert_eq!(
        fixture_inventory["db_row_counts"]["agents"].as_u64(),
        Some(1)
    );
    assert_eq!(
        fixture_inventory["db_row_counts"]["conversations"].as_u64(),
        Some(1)
    );
    assert_eq!(
        fixture_inventory["db_row_counts"]["messages"].as_u64(),
        Some(2)
    );
    assert!(
        fixture_inventory["mirror_hash_inventory"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "fixture inventory should include raw mirror hash evidence"
    );
    let inventory_text =
        serde_json::to_string(&fixture_inventory).expect("serialize fixture inventory");
    assert!(
        !inventory_text.contains(temp.path().to_string_lossy().as_ref()),
        "fixture inventory should redact temp paths"
    );
    assert!(
        !inventory_text.contains("CASS_DOCTOR_PRIVACY_SENTINEL"),
        "fixture inventory should not leak privacy sentinels"
    );

    let source_before: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("source-inventory-before.json")).unwrap(),
    )
    .expect("source inventory before json");
    let source_after: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("source-inventory-after.json")).unwrap(),
    )
    .expect("source inventory after json");
    assert_eq!(source_before["phase"].as_str(), Some("before"));
    assert_eq!(source_after["phase"].as_str(), Some("after"));
    assert!(
        source_before["raw_mirror_files"]["tree_entry_count"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "before source inventory should include raw mirror file evidence"
    );
    assert_eq!(
        source_before["raw_mirror_files"]["tree_entry_count"],
        source_after["raw_mirror_files"]["tree_entry_count"],
        "read-only doctor run should not change raw mirror inventory"
    );

    let execution_flow =
        std::fs::read_to_string(result.artifact_dir.join("execution-flow.jsonl")).unwrap();
    for phase in [
        "source_discovery",
        "raw_mirror_hash",
        "parse_outcome",
        "db_projection_outcome",
        "source_inventory_before",
        "source_inventory_after",
        "mutation_audit",
    ] {
        assert!(
            execution_flow.contains(&format!("\"phase\":\"{phase}\"")),
            "execution flow should include phase {phase}: {execution_flow}"
        );
    }
    assert!(
        execution_flow.contains("\"doctor_command\"")
            && execution_flow.contains("\"surface\":\"check\""),
        "execution flow should record that read-only scenarios exercise doctor check: {execution_flow}"
    );
}

#[test]
fn doctor_e2e_runner_records_truncated_source_with_verified_mirror() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let runner = DoctorE2eRunner::new(temp.path().join("run")).expect("runner");
    let spec = DoctorE2eScenarioSpec::new(
        "artifact-source-truncated",
        DoctorFixtureScenario::SourceTruncated,
        ["quick", "source-mirror", "truncated"],
    )
    .require_json_pointer("/source_inventory")
    .require_json_pointer("/raw_mirror")
    .require_json_pointer("/coverage_summary")
    .require_json_pointer("/source_authority/selected_authority");

    let result = runner
        .run_scenario(&spec)
        .expect("run truncated-source doctor e2e scenario");
    assert_eq!(result.status, "pass");
    validate_artifact_manifest(&result.manifest_path).expect("artifact manifest valid");

    let source_before: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("source-inventory-before.json")).unwrap(),
    )
    .expect("source inventory before json");
    let source_after: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("source-inventory-after.json")).unwrap(),
    )
    .expect("source inventory after json");
    assert!(
        source_before["upstream_source_files"]["tree_entry_count"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "truncated-source fixture should keep the upstream path present"
    );
    assert_eq!(
        source_before["source_discovery"]["expected_missing_current_source_count"].as_u64(),
        Some(0),
        "truncated source is degraded evidence, not a missing-source fixture"
    );
    assert_eq!(
        source_before["raw_mirror_files"]["tree_entry_count"],
        source_after["raw_mirror_files"]["tree_entry_count"],
        "read-only truncated-source check must not rewrite raw mirror evidence"
    );
    let structured_log = source_before["source_discovery"]["structured_fixture_log"]
        .as_array()
        .expect("structured fixture log");
    assert!(
        structured_log.iter().any(|entry| {
            entry["step"].as_str() == Some("overwrite_file_for_fixture_drift")
                && entry["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.contains("provider_source_truncated"))
        }),
        "fixture log should prove the upstream source was truncated after mirror capture: {structured_log:#?}"
    );

    let payload: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("parsed-json/doctor-json.json")).unwrap(),
    )
    .expect("doctor json artifact");
    assert_eq!(
        payload["source_inventory"]["missing_current_source_count"].as_u64(),
        Some(0),
        "doctor should distinguish present-but-truncated sources from removed sources"
    );
    assert_eq!(payload["raw_mirror"]["status"].as_str(), Some("verified"));
    assert_eq!(
        payload["raw_mirror"]["manifests"][0]["upstream_path_exists"].as_bool(),
        Some(true),
        "raw mirror report should record that the upstream path still exists"
    );
    assert_eq!(
        payload["coverage_summary"]["raw_mirror_db_link_count"].as_u64(),
        Some(1),
        "coverage summary should keep the verified mirror link after source truncation"
    );
    let stdout =
        std::fs::read_to_string(result.artifact_dir.join("stdout/doctor-json.out")).unwrap();
    assert!(
        !stdout.contains("truncated after mirror"),
        "doctor JSON must not leak truncated source bytes"
    );

    let execution_flow =
        std::fs::read_to_string(result.artifact_dir.join("execution-flow.jsonl")).unwrap();
    for field in [
        "source_discovery",
        "raw_mirror_hash",
        "source_inventory_before",
        "source_inventory_after",
        "mutation_audit",
    ] {
        assert!(
            execution_flow.contains(field),
            "truncated-source execution flow should include {field}: {execution_flow}"
        );
    }
}

#[test]
fn doctor_e2e_runner_reports_no_safe_rebuild_authority_without_mirror() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let runner = DoctorE2eRunner::new(temp.path().join("run")).expect("runner");
    let spec = DoctorE2eScenarioSpec::new(
        "artifact-mirror-missing",
        DoctorFixtureScenario::MirrorMissing,
        ["quick", "source-mirror", "fault"],
    )
    .require_json_pointer("/source_inventory")
    .require_json_pointer("/raw_mirror")
    .require_json_pointer("/coverage_summary")
    .require_json_pointer("/coverage_risk")
    .require_json_pointer("/source_authority")
    .require_json_pointer("/candidate_staging");

    let result = runner
        .run_scenario(&spec)
        .expect("run mirror-missing doctor e2e scenario");
    assert_eq!(result.status, "pass");
    validate_artifact_manifest(&result.manifest_path).expect("artifact manifest valid");

    let payload: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("parsed-json/doctor-json.json")).unwrap(),
    )
    .expect("doctor json artifact");
    assert_eq!(
        payload["source_inventory"]["missing_current_source_count"].as_u64(),
        Some(1),
        "mirror-missing fixture should report the pruned upstream source"
    );
    assert_eq!(
        payload["raw_mirror"]["summary"]["manifest_count"].as_u64(),
        Some(0),
        "mirror-missing fixture should not invent raw mirror manifests"
    );
    assert_eq!(
        payload["coverage_summary"]["db_without_raw_mirror_count"].as_u64(),
        Some(1),
        "coverage summary should flag archive rows without mirror evidence"
    );
    assert_eq!(
        payload["coverage_summary"]["coverage_reducing_live_source_rebuild_refused"].as_bool(),
        Some(true),
        "doctor must refuse source-session-only rebuild when it would shrink coverage"
    );
    let selected_authorities = payload["source_authority"]["selected_authorities"]
        .as_array()
        .expect("selected authorities");
    assert!(
        selected_authorities
            .iter()
            .all(|candidate| candidate["authority"].as_str() != Some("verified_raw_mirror")),
        "verified raw mirror must not be selected when no mirror exists: {:#}",
        payload["source_authority"]
    );
    assert!(
        payload["source_authority"]["rejected_authorities"]
            .as_array()
            .expect("rejected authorities")
            .iter()
            .any(|candidate| {
                candidate["authority"].as_str() == Some("live_upstream_source")
                    && candidate["evidence"].as_array().is_some_and(|evidence| {
                        evidence.iter().any(|entry| {
                            entry.as_str() == Some("coverage-shrinks-relative-to-archive")
                        })
                    })
            }),
        "live upstream source should be rejected with coverage-shrink evidence: {:#}",
        payload["source_authority"]
    );
    assert!(
        payload["candidate_staging"]["latest_build"].is_null(),
        "read-only mirror-missing check should not stage a candidate"
    );

    let source_before: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("source-inventory-before.json")).unwrap(),
    )
    .expect("source inventory before json");
    assert_eq!(
        source_before["raw_mirror_files"]["tree_entry_count"].as_u64(),
        Some(0),
        "source inventory should prove there were no raw mirror files"
    );
    let execution_flow =
        std::fs::read_to_string(result.artifact_dir.join("execution-flow.jsonl")).unwrap();
    assert!(
        execution_flow.contains("\"status\":\"unchanged\""),
        "mirror-missing read-only run should preserve no-mutation evidence: {execution_flow}"
    );
}

#[test]
fn doctor_e2e_runner_builds_candidate_with_fix_and_logs_lifecycle() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let runner = DoctorE2eRunner::new(temp.path().join("run")).expect("runner");
    let spec = DoctorE2eScenarioSpec::new(
        "artifact-candidate-build",
        DoctorFixtureScenario::SourcePruned,
        ["candidate", "source-mirror"],
    )
    .allow_mutation(true)
    .require_json_pointer("/candidate_staging")
    .require_json_pointer("/candidate_staging/latest_build")
    .require_json_pointer("/candidate_staging/latest_build/candidate_id")
    .require_json_pointer("/candidate_staging/latest_build/live_inventory_before")
    .require_json_pointer("/candidate_staging/latest_build/live_inventory_after")
    .require_json_pointer("/candidate_staging/latest_build/manifest_path");

    let result = runner
        .run_scenario(&spec)
        .expect("run candidate-build doctor e2e scenario");
    assert_eq!(result.status, "pass");
    validate_artifact_manifest(&result.manifest_path).expect("artifact manifest valid");

    let candidate_staging: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("candidate-staging.json")).unwrap(),
    )
    .expect("candidate staging artifact json");
    let latest_build = &candidate_staging["latest_build"];
    assert_eq!(
        latest_build["status"].as_str(),
        Some("completed"),
        "mutating doctor e2e should build a terminal candidate: {candidate_staging:#}"
    );
    assert!(
        latest_build["candidate_id"]
            .as_str()
            .is_some_and(|id| !id.trim().is_empty()),
        "candidate build should record a stable candidate_id: {candidate_staging:#}"
    );
    assert_eq!(
        latest_build["candidate_conversation_count"].as_u64(),
        Some(1),
        "candidate DB should preserve the fixture conversation row"
    );
    assert_eq!(
        latest_build["candidate_message_count"].as_u64(),
        Some(2),
        "candidate DB should preserve fixture messages"
    );
    assert_eq!(
        latest_build["live_inventory_unchanged"].as_bool(),
        Some(true),
        "candidate build must prove live DB/index inventory is unchanged before any promotion"
    );
    assert!(
        latest_build["checksum_count"]
            .as_u64()
            .is_some_and(|count| count >= 6),
        "candidate should checksum DB, logs, receipts, and derived metadata: {candidate_staging:#}"
    );
    assert!(
        latest_build["selected_authority_evidence"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item
                .as_str()
                .is_some_and(|text| text.starts_with("verified-blob-count=")))),
        "candidate e2e should prove raw mirror evidence contributed to the authority decision"
    );
    assert_eq!(
        candidate_staging["completed_candidate_count"].as_u64(),
        Some(1),
        "candidate staging inventory should report the completed candidate"
    );

    let after_tree: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("file-tree-after.json")).unwrap(),
    )
    .expect("after file tree json");
    let data_entries = after_tree["roots"]
        .as_array()
        .and_then(|roots| {
            roots
                .iter()
                .find(|root| root["root_id"].as_str() == Some("data"))
        })
        .and_then(|root| root["entries"].as_array())
        .expect("data tree entries");
    for expected_suffix in [
        "manifest.json",
        "database/candidate.db",
        "logs/skipped-records.jsonl",
        "logs/parse-errors.jsonl",
        "receipts/fs-mutations.jsonl",
        "index/lexical/candidate-generation.json",
        "index/semantic/metadata.json",
    ] {
        assert!(
            data_entries.iter().any(|entry| {
                entry["relative_path"].as_str().is_some_and(|path| {
                    path.starts_with("doctor/candidates/") && path.ends_with(expected_suffix)
                })
            }),
            "candidate file tree should include {expected_suffix}: {after_tree:#}"
        );
    }

    let execution_flow =
        std::fs::read_to_string(result.artifact_dir.join("execution-flow.jsonl")).unwrap();
    assert!(
        execution_flow.contains("\"phase\":\"candidate_staging\""),
        "execution flow should include a candidate_staging phase: {execution_flow}"
    );
    for field in [
        "candidate_id",
        "lifecycle_status",
        "manifest_path",
        "checksum_count",
        "skipped_record_count",
        "parse_error_count",
        "selected_authority",
        "evidence_sources",
        "coverage_before",
        "coverage_after",
        "confidence",
        "live_inventory_before",
        "live_inventory_after",
        "live_inventory_unchanged",
    ] {
        assert!(
            execution_flow.contains(field),
            "candidate e2e log should include {field}: {execution_flow}"
        );
    }
}

#[test]
fn doctor_e2e_runner_reconstructs_candidate_from_mirror_when_db_is_corrupt() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let runner = DoctorE2eRunner::new(temp.path().join("run")).expect("runner");
    let spec = DoctorE2eScenarioSpec::new(
        "artifact-corrupt-db-mirror-reconstruct",
        DoctorFixtureScenario::DbCorrupt,
        ["candidate", "archive-corrupt", "source-mirror"],
    )
    .allow_mutation(true)
    .require_json_pointer("/raw_mirror")
    .require_json_pointer("/candidate_staging/latest_build")
    .require_json_pointer("/candidate_staging/latest_build/evidence_sources")
    .require_json_pointer("/candidate_staging/latest_build/coverage_before")
    .require_json_pointer("/candidate_staging/latest_build/coverage_after")
    .require_json_pointer("/candidate_staging/latest_build/confidence");

    let result = runner
        .run_scenario(&spec)
        .expect("run corrupt-db mirror reconstruction scenario");
    assert_eq!(result.status, "pass");
    validate_artifact_manifest(&result.manifest_path).expect("artifact manifest valid");

    let candidate_staging: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("candidate-staging.json")).unwrap(),
    )
    .expect("candidate staging artifact json");
    let latest_build = &candidate_staging["latest_build"];
    assert_eq!(latest_build["status"].as_str(), Some("completed"));
    assert_eq!(
        latest_build["confidence"].as_str(),
        Some("verified_raw_mirror_reconstruction")
    );
    assert_eq!(
        latest_build["candidate_conversation_count"].as_u64(),
        Some(1)
    );
    assert_eq!(latest_build["candidate_message_count"].as_u64(), Some(1));
    assert!(
        latest_build["evidence_sources"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item
                .as_str()
                .is_some_and(|text| text.starts_with("verified_raw_mirror:manifest_id=")))),
        "candidate build should identify verified raw mirror evidence: {latest_build:#}"
    );
    assert_eq!(
        latest_build["coverage_after"]["coverage_source"].as_str(),
        Some("verified_raw_mirror_candidate_archive")
    );
    assert_eq!(
        latest_build["live_inventory_unchanged"].as_bool(),
        Some(true),
        "candidate build must not overwrite the corrupt live archive"
    );

    let after_tree: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("file-tree-after.json")).unwrap(),
    )
    .expect("after file tree json");
    let data_entries = after_tree["roots"]
        .as_array()
        .and_then(|roots| {
            roots
                .iter()
                .find(|root| root["root_id"].as_str() == Some("data"))
        })
        .and_then(|root| root["entries"].as_array())
        .expect("data tree entries");
    assert!(
        data_entries.iter().any(|entry| {
            entry["relative_path"].as_str().is_some_and(|path| {
                path.starts_with("doctor/candidates/")
                    && path.contains("/evidence/raw-mirror/blobs/")
            })
        }),
        "candidate should stage raw mirror evidence copies for audit: {after_tree:#}"
    );
    let corrupt_db_after = data_entries
        .iter()
        .find(|entry| entry["relative_path"].as_str() == Some("agent_search.db"))
        .expect("live corrupt DB entry");
    assert_eq!(
        corrupt_db_after["size_bytes"].as_u64(),
        Some("not a sqlite database".len() as u64),
        "live corrupt DB should remain in place for later explicit promotion/restore handling"
    );
}

#[test]
fn doctor_e2e_runner_records_multi_file_source_artifacts() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let runner = DoctorE2eRunner::new(temp.path().join("run")).expect("runner");
    let spec = DoctorE2eScenarioSpec::new(
        "artifact-multi-file-source",
        DoctorFixtureScenario::MultiSource,
        ["source-mirror", "multi-file"],
    )
    .require_json_pointer("/source_inventory")
    .require_json_pointer("/source_inventory/provider_counts/codex")
    .require_json_pointer("/source_inventory/provider_counts/cline")
    .require_json_pointer("/operation_outcome/kind")
    .require_json_pointer("/source_authority/selected_authority");

    let result = runner
        .run_scenario(&spec)
        .expect("run multi-file doctor e2e scenario");
    assert_eq!(result.status, "pass");
    validate_artifact_manifest(&result.manifest_path).expect("artifact manifest valid");

    let source_before: serde_json::Value = serde_json::from_slice(
        &std::fs::read(result.artifact_dir.join("source-inventory-before.json")).unwrap(),
    )
    .expect("source inventory before json");
    assert_eq!(
        source_before["source_discovery"]["provider_set"]
            .as_array()
            .map(Vec::len),
        Some(2),
        "multi-source artifact should record both fixture providers"
    );
    assert_eq!(
        source_before["source_discovery"]["expected_provider_counts"]["codex"].as_u64(),
        Some(1)
    );
    assert_eq!(
        source_before["source_discovery"]["expected_provider_counts"]["cline"].as_u64(),
        Some(1)
    );
    assert!(
        source_before["upstream_source_files"]["tree_entry_count"]
            .as_u64()
            .is_some_and(|count| count >= 3),
        "multi-file source inventory should include Codex primary, Cline primary, and Cline sidecar"
    );
    let source_artifacts = source_before["upstream_source_files"]["artifacts"]
        .as_array()
        .expect("source artifacts array");
    assert!(
        source_artifacts.iter().any(|artifact| {
            artifact["artifact_kind"].as_str() == Some("provider_source_sidecar")
                && artifact["relative_path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("task_metadata.json"))
        }),
        "multi-file source artifact bundle should include the Cline metadata sidecar"
    );

    let execution_flow =
        std::fs::read_to_string(result.artifact_dir.join("execution-flow.jsonl")).unwrap();
    for phase in [
        "source_discovery",
        "parse_outcome",
        "db_projection_outcome",
        "source_inventory_before",
        "source_inventory_after",
    ] {
        assert!(
            execution_flow.contains(&format!("\"phase\":\"{phase}\"")),
            "multi-file execution flow should include phase {phase}: {execution_flow}"
        );
    }
}

#[test]
fn doctor_e2e_intentional_failure_preserves_failure_context_and_artifacts() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let runner = DoctorE2eRunner::new(temp.path().join("run")).expect("runner");
    let spec = DoctorE2eScenarioSpec::new(
        "intentional-failure",
        DoctorFixtureScenario::SourcePruned,
        ["quick", "self-test"],
    )
    .require_json_pointer("/definitely_missing_for_self_test");

    let result = runner
        .run_scenario(&spec)
        .expect("runner should return a failed result with artifacts");
    assert_eq!(result.status, "fail");
    let context = result.failure_context.expect("failure context");
    assert!(
        context
            .reasons
            .iter()
            .any(|reason| reason.contains("required JSON pointer")),
        "failure context should explain the assertion failure: {:?}",
        context.reasons
    );
    assert!(result.artifact_dir.join("failure_summary.txt").exists());
    validate_artifact_manifest(&result.manifest_path).expect("failed artifact manifest valid");
}

#[test]
fn doctor_e2e_scripted_scenarios() {
    let labels = std::env::var("CASS_DOCTOR_E2E_LABELS").unwrap_or_else(|_| "quick".to_string());
    let scenarios_arg = std::env::var("CASS_DOCTOR_E2E_SCENARIOS").unwrap_or_default();
    let mut args = vec!["doctor_v2".to_string(), "--label".to_string(), labels];
    if !scenarios_arg.trim().is_empty() {
        args.push("--scenario".to_string());
        args.push(scenarios_arg);
    }
    if std::env::var("CASS_DOCTOR_E2E_INCLUDE_FAILURE_SELF_TEST").is_ok() {
        args.push("--include-failure-self-test".to_string());
    }
    let parsed = DoctorE2eCliArgs::parse_from(args).expect("parse scripted args");
    let scenarios = doctor_e2e_scenarios_for_args(&parsed);
    let selected = select_scenarios(&parsed, &scenarios);
    assert!(
        !selected.is_empty(),
        "doctor e2e script selection should choose at least one scenario"
    );

    let run_root = std::env::var("CASS_DOCTOR_E2E_RUN_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| default_doctor_e2e_run_root());
    let runner = DoctorE2eRunner::new(&run_root).expect("runner");
    for scenario in selected {
        let result = runner
            .run_scenario(scenario)
            .expect("run scripted scenario");
        assert_eq!(
            result.status,
            scenario.expected_runner_status(),
            "scripted doctor scenario should produce the expected status with artifacts at {}",
            result.artifact_dir.display()
        );
        if parsed.fail_fast && result.status == "fail" {
            break;
        }
    }
}
