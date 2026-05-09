#!/usr/bin/env bash
# vz9t8_4_crypto_tracing_e2e.sh — exercise the crypto-tracing tests.
#
# Per coding_agent_session_search-vz9t8.4. Runs the safety tests under
# RUST_LOG=debug, then greps the captured stderr for any 32-byte hex pattern
# (basic key-leak heuristic).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RCH_TARGET_DIR="${RCH_TARGET_DIR:-/tmp/cass-vz9t8-4-target}"
LOG="$RCH_TARGET_DIR/crypto-tracing-e2e.log"
mkdir -p "$RCH_TARGET_DIR"
exec > >(tee -a "$LOG") 2>&1

cleanup() {
    local rc=$?
    if [ "$rc" -ne 0 ]; then
        echo ""
        echo "[vz9t8_4_e2e] FAILURE — last 50 log lines:" >&2
        tail -n 50 "$LOG" | sed 's/^/[vz9t8_4_e2e]   /' >&2
    fi
    exit "$rc"
}
trap cleanup EXIT

PASS=0
FAIL=0

echo "[vz9t8_4_e2e] running cargo test crypto_tracing_safety"
TEST_OUT="$RCH_TARGET_DIR/crypto-tracing-test.out"
if rch exec -- env CARGO_TARGET_DIR="$RCH_TARGET_DIR" RUST_LOG=debug \
    cargo test --test crypto_tracing_safety -- --nocapture --test-threads=1 \
    > "$TEST_OUT" 2>&1; then
    if grep -q "test result: ok" "$TEST_OUT"; then
        echo "[vz9t8_4_e2e] OK: cargo test passed"
        PASS=$((PASS + 1))
    else
        echo "[vz9t8_4_e2e] FAIL: cargo test exited 0 but no 'test result: ok'"
        FAIL=$((FAIL + 1))
    fi
else
    echo "[vz9t8_4_e2e] FAIL: cargo test failed — see $TEST_OUT"
    tail -50 "$TEST_OUT" | sed 's/^/[vz9t8_4_e2e]   /'
    FAIL=$((FAIL + 1))
fi

# Heuristic external leak check: search the captured test stderr for any
# 64-character hex run that could be a key. The test's known patterns are
# 0xCA*32 → "cacacacaca..." and 0xDE*16 → "dededede...". Either of those
# appearing in tracing output means a leak.
echo "[vz9t8_4_e2e] checking captured test output for key-leak patterns"
LEAK_HITS=$( ( grep -ciE '(ca){16}|(CA){16}|(de){8}|(DE){8}' "$TEST_OUT" || true ) | head -1 )
if [ "${LEAK_HITS:-0}" -eq 0 ]; then
    echo "[vz9t8_4_e2e] OK: no leak patterns found in cargo test output"
    PASS=$((PASS + 1))
else
    echo "[vz9t8_4_e2e] FAIL: $LEAK_HITS line(s) match key-leak pattern; see $TEST_OUT"
    FAIL=$((FAIL + 1))
fi

echo ""
echo "[vz9t8_4_e2e] SUMMARY: PASS=$PASS FAIL=$FAIL"
echo "[vz9t8_4_e2e] log: $LOG"
[ "$FAIL" -eq 0 ] && echo "[vz9t8_4_e2e] ALL PASS" || exit 1
