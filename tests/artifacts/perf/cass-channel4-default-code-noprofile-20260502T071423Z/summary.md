# CASS lexical rebuild channel-depth perf slice

Date: 2026-05-02

## Workload

- Command shape: `cass index --watch-once <nonexistent> --data-dir <seeded-data-dir> --json --progress-interval-ms 5000`
- Seed database: `/home/ubuntu/cass-post-tokenizer-hotspot-20260502T035907Z/agent_search.db*`
- Corpus: 51,214 conversations / 4,711,459 messages
- Binary: `/data/tmp/cass_perf_opt_target/profiling/cass`
- Changed default: `CASS_TANTIVY_REBUILD_PIPELINE_CHANNEL_SIZE` fallback `2 -> 4`

## Baseline

Artifact: `tests/artifacts/perf/cass-clean-default-noprofile-20260502T065520Z`

- `elapsed_ms`: 44,731
- Wall time: 0:45.54
- Full corpus reached: 34,523 ms
- Phase returned to preparing: 39,727 ms
- Max RSS: 61,011,208 KB
- File system outputs: 17,668,376 KB
- Producer handoff wait: 1,122 waits / 4,978 ms

## Candidate

Artifact: `tests/artifacts/perf/cass-channel4-default-code-noprofile-20260502T071423Z`

- `elapsed_ms`: 43,531
- Wall time: 0:44.73
- Full corpus reached: 34,424 ms
- Phase returned to preparing: 39,928 ms
- Max RSS: 60,446,128 KB
- File system outputs: 17,493,840 KB
- Producer handoff wait: 938 waits / 4,198 ms

## Delta

- Total `elapsed_ms`: 2.7% faster (`44,731 -> 43,531`)
- Wall time: 1.8% faster (`45.54s -> 44.73s`)
- Producer handoff wait: 15.7% fewer waits and 15.7% fewer wait ms
- RSS: effectively unchanged (`61,011,208 KB -> 60,446,128 KB`)
- File system outputs: 1.0% lower (`17,668,376 KB -> 17,493,840 KB`)

## A/B notes

- `CASS_TANTIVY_REBUILD_PIPELINE_CHANNEL_SIZE=4` before the code change produced the strongest timing sample: `elapsed_ms=42,433`, wall `43.74s`, max RSS `60,721,700 KB`.
- `CASS_TANTIVY_REBUILD_PIPELINE_CHANNEL_SIZE=8` regressed to `elapsed_ms=44,035`, so the default should not jump past 4.
- `CASS_TANTIVY_REBUILD_PAGE_PREP_WORKERS=8` was latency-neutral but lowered RSS (`61,011,208 KB -> 46,154,916 KB`); useful future memory lever, not this speed slice.
- Dense range SQL batch fetch was rejected: `tests/artifacts/perf/cass-range-batch-noprofile-20260502T062500Z` stalled for 120s, timed out at 160s, and peaked at `241,863,448 KB` RSS.

## Interpretation

After the final-frontier publish optimization, the remaining foreground cost includes ordered producer-to-sink handoff stalls while shard builders consume prepared pages. A channel depth of 4 keeps the bounded backpressure model but gives the producer enough slack to overlap page prep, shard build dispatch, and eager merge work. A depth of 8 allows larger transient queues and trips more pressure-controller churn without improving throughput.

## Behavior proof

- Ordering preserved: yes. The channel only buffers already sequenced `LexicalRebuildPipelineMessage::Batch` items; ordered page emission remains in the producer.
- Document set preserved: yes. The same prepared pages and shard boundaries are consumed; only bounded handoff slack changes.
- Fallback/operator control: yes. `CASS_TANTIVY_REBUILD_PIPELINE_CHANNEL_SIZE` still overrides the default.
- Query smoke: `function` on baseline and candidate both returned `total_matches=241394` with the same top 5 hits.

## Verification

- `cargo test -q lexical_rebuild_pipeline_settings_snapshot --lib`
- Search smoke against baseline and candidate data dirs for `function`
- `cargo fmt --check`
- `cargo check --all-targets`
- `cargo clippy --all-targets -- -D warnings`
