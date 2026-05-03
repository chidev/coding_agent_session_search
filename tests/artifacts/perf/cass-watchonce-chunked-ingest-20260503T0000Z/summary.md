# cass watch-once ingest memory pass - 2026-05-03

Workload:

```bash
timeout 90s cass index \
  --watch-once /home/ubuntu/.codex/sessions/2026/05/02/rollout-2026-05-02T07-34-40-019de878-1fb0-7ad1-b7b0-b8c9a80769fa.jsonl \
  --data-dir <fresh-data-dir> \
  --json --progress-interval-ms 5000 --color=never
```

All rows used the profiling binary at:

```text
/data/tmp/cass-target-watchonce-chunk-20260503T0000Z/profiling/cass
```

## Results

| Run | Code shape | Wall | Max RSS KB | Exit | Conversations | Messages | Message bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `baseline` | pre-chunking baseline | `1:30.35` | `2,745,832` | `124` | `248` | `18,825` | `9,031,461` |
| `after` | chunked explicit watch-once, default 64 | `1:30.42` | `2,852,012` | `124` | `376` | `23,356` | `10,153,325` |
| `after-chunk1` | chunked explicit watch-once, env chunk size 1 | `1:30.38` | `2,703,532` | `124` | `307` | `24,948` | `11,187,832` |
| `after-move` | move mapped `Conversation`s into writer prep, no chunking | `1:30.34` | `2,372,508` | `124` | `312` | `21,261` | `9,464,045` |

## Decision

The conversation-chunking lever did not prove the intended memory claim: the default chunked run raised max RSS versus baseline, and chunk size 1 traded progress shape for lower RSS without completing the workload.

The retained change is the ownership move in `persist_conversations_batched_inner`: after `map_to_internal` builds owned `Conversation`s, writer preparation now consumes them with `into_iter()` instead of cloning every conversation and message payload a second time. This reduces peak RSS by about 13.6% versus baseline while also increasing partial work completed within the fixed 90-second window.
