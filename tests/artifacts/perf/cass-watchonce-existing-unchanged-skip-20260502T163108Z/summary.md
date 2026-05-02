# cass watch-once existing-file unchanged skip

Date: 2026-05-02

Workload:

```bash
/data/tmp/cargo-target/profiling/cass index \
  --watch-once /home/ubuntu/.codex/sessions/2025/12/17/rollout-2025-12-17T16-36-28-019b2e3e-3972-7390-b77f-a90f83498bff.jsonl \
  --data-dir /tmp/cass-watchonce-existing-unchanged-skip-final2-20260502T164747Z \
  --json --progress-interval-ms 5000
```

Seed data dir: `/home/ubuntu/cass-post-tokenizer-hotspot-20260502T035907Z`

## Result

| Run | JSON elapsed_ms | wall | max RSS |
| --- | ---: | ---: | ---: |
| Prior shipped targeted fast path | 49944 | 0:50.14 | 15633088 KB |
| Rejected split lookup probe | 49423 | 0:49.64 | 15710140 KB |
| Point-probe fallback only | 19890 | 0:20.11 | 8710456 KB |
| Final unchanged-file skip | 1946 | 0:02.10 | 1366860 KB |

The final path skips explicit watch-once ingestion when the target file is already represented in canonical storage and its file mtime is older than `last_indexed_at`.

## Files

- `index.out.json`: final robot output.
- `index.stderr.txt`: final progress and `/usr/bin/time -v` output.
