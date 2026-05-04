# cass lexical page-prep worker ceiling probe - 2026-05-04

## Workload

Copied production-sized CASS DB from `/home/ubuntu/cass-shard-plan-tail-estimate-20260504T051722Z/agent_search.db` into isolated data dirs and ran:

```bash
CASS_RESPONSIVENESS_DISABLE=1 \
CASS_PREP_PROFILE=1 \
CASS_TANTIVY_REBUILD_PROFILE=1 \
<profiling-cass> index --watch-once <codex rollout jsonl> \
  --data-dir <isolated-data-dir> \
  --json --progress-interval-ms 5000 --color=never
```

## Results

| Row | Page prep workers | CLI elapsed | Wall time | Max RSS | plan_lexical_shards |
| --- | ---: | ---: | ---: | ---: | ---: |
| default-before | 6 | 42,930 ms | 44.43 s | 40,291,572 KB | 3,140 ms |
| candidate | 8 | 40,928 ms | 42.23 s | 46,015,280 KB | 3,080 ms |
| candidate | 7 | 42,429 ms | 43.53 s | 43,055,628 KB | 3,154 ms |

The 8-worker ceiling was the only measured fanout row with a material wall-time win after the prior missing-tail-state streaming fix moved `plan_lexical_shards` out of the critical path. It improves CLI elapsed by 2,002 ms (4.7%) and wall time by 2.20 s (5.0%) versus the current 6-worker default on this copied-DB workload.

## Decision

Raise the large-host default page-prep ceiling from 6 to 8. The tradeoff is about +5.7 GB peak RSS on this workload, but the new peak remains below the pre-tail-state-fix runs and the responsiveness governor can still scale the requested worker count down under host pressure. Operators can override with `CASS_TANTIVY_REBUILD_PAGE_PREP_WORKERS` when they need a stricter memory envelope.
