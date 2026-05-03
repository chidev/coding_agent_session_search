# CASS lexical rebuild staged-merge scheduler budget - 2026-05-03

## Result

Kept. Current `HEAD` (`78622abc`, including production commit `cd3a7daf`) indexes the seeded watch-once workload faster than the accepted fan-in 8 baseline:

| Variant | CLI elapsed_ms | wall time | max RSS KB | FS outputs |
| --- | ---: | ---: | ---: | ---: |
| fan-in 8 accepted baseline (`986271ae`) | 56848 | 0:58.15 | 40547200 | 14641104 |
| fan-in 16 rejected (`71981faa`) | 77148 | 1:19.59 | 40418224 | 14452808 |
| fan-in 8 + saturation merge budget (`78622abc`) | 48659 | 0:49.44 | 39810508 | 15390800 |

The accepted change is 14.4% faster by CLI elapsed time and 15.0% faster by wall clock versus the accepted fan-in 8 baseline. The fan-in 16 attempt was rejected after the seeded workload regressed to 77.148s CLI elapsed / 1:19.59 wall.

## What changed

The staged merge controller now spends a bounded merge budget while page-prep workers are saturated, instead of capping staged merge dispatch to one job whenever `active_page_prep_jobs >= page_prep_workers`. The budget is capped to half of staged merge workers and existing active merge jobs are preserved. The production code also restores `LexicalRebuildShardMergeCoordinator::EAGER_MERGE_FAN_IN` to 8 because the measured fan-in 16 row regressed.

## Command

Seed:

```bash
cp --reflink=auto /home/ubuntu/cass-lexical-merge-fanin8-20260503T190615Z/agent_search.db /home/ubuntu/cass-lexical-merge-scheduler-budget-20260503T203956Z/agent_search.db
```

Benchmark:

```bash
/usr/bin/time -v -o tests/artifacts/perf/cass-lexical-merge-scheduler-budget-20260503T203956Z/scheduler-budget.time.txt \
  timeout 140s env CASS_RESPONSIVENESS_DISABLE=1 CASS_PREP_PROFILE=1 \
  /data/tmp/cass-target-summary-footprints-20260503/profiling/cass \
  index --watch-once /home/ubuntu/.codex/sessions/2026/05/02/rollout-2026-05-02T18-41-41-019deada-cd88-74e3-b215-90094437fbc0.jsonl \
  --data-dir /home/ubuntu/cass-lexical-merge-scheduler-budget-20260503T203956Z \
  --json --progress-interval-ms 5000 --color=never \
  > tests/artifacts/perf/cass-lexical-merge-scheduler-budget-20260503T203956Z/scheduler-budget.out.json \
  2> tests/artifacts/perf/cass-lexical-merge-scheduler-budget-20260503T203956Z/scheduler-budget.stderr.txt
```

## Evidence

- `scheduler-budget.out.json`: `success=true`, `elapsed_ms=48659`.
- `scheduler-budget.time.txt`: wall `0:49.44`, max RSS `39810508` KB, FS outputs `15390800`.
- `scheduler-budget.stderr.txt`: `CASS_PREP_PROFILE step=plan_lexical_shards step_ms=3058`; progress snapshots show staged merge budgets rising under saturation, for example `page_prep_workers_saturated_6_of_6_merge_budget_4_active_jobs_4_ready_groups_3`.
