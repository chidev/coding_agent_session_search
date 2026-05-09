#!/usr/bin/env bash
# 50m1v_degradation_tier_contract_e2e.sh — exercise degradation tier contract.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RCH_TARGET_DIR="${RCH_TARGET_DIR:-/tmp/cass-50m1v-target}"
LOG="$RCH_TARGET_DIR/degradation-tier-contract-e2e.log"
mkdir -p "$RCH_TARGET_DIR"
exec > >(tee -a "$LOG") 2>&1

cleanup() {
    local rc=$?
    if [ "$rc" -ne 0 ]; then
        tail -n 50 "$LOG" >&2
    fi
    exit "$rc"
}
trap cleanup EXIT

echo "[50m1v_e2e] running cargo test degradation_tier_contract"
TEST_OUT="$RCH_TARGET_DIR/test.out"
PASS=0; FAIL=0
if rch exec -- env CARGO_TARGET_DIR="$RCH_TARGET_DIR" \
    cargo test --test degradation_tier_contract -- --nocapture > "$TEST_OUT" 2>&1; then
    if grep -q "test result: ok" "$TEST_OUT"; then
        echo "[50m1v_e2e] OK: contract tests passed"
        PASS=$((PASS + 1))
    else
        echo "[50m1v_e2e] FAIL: cargo exited 0 but no 'test result: ok'"
        FAIL=$((FAIL + 1))
    fi
else
    echo "[50m1v_e2e] FAIL: cargo test failed"
    tail -50 "$TEST_OUT" | sed 's/^/[50m1v_e2e]   /'
    FAIL=$((FAIL + 1))
fi

echo "[50m1v_e2e] SUMMARY: PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ] && echo "[50m1v_e2e] ALL PASS" || exit 1
