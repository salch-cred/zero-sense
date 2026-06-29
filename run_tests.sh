#!/usr/bin/env bash
# ZeroSense Full Test Runner — bash run_tests.sh
set -e
PASS=0; FAIL=0

echo ""
echo "🤖 ZeroSense — Full Test Suite"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ─── PYTHON ────────────────────────────────
echo ""; echo "🐍 PYTHON TESTS"
if python -m pytest tests/ -v --tb=short 2>&1; then
    echo "✅ Python tests PASSED"; PASS=$((PASS+1))
else
    echo "❌ Python tests FAILED"; FAIL=$((FAIL+1))
fi

# ─── RUST CONTRACTS ────────────────────────
echo ""; echo "🦀 SOROBAN CONTRACT TESTS"
for contract in verifier payment reputation insurance fleet_identity; do
    echo "  Testing: $contract"
    if (cd contracts/$contract && cargo test 2>&1); then
        echo "  ✅ $contract PASSED"; PASS=$((PASS+1))
    else
        echo "  ❌ $contract FAILED"; FAIL=$((FAIL+1))
    fi
done

# ─── ZKVM BUILD ────────────────────────────
echo ""; echo "🔐 ZKVM BUILD CHECK"
for pkg in host guest; do
    if (cd zkvm/$pkg && cargo build 2>&1); then
        echo "  ✅ zkvm/$pkg OK"; PASS=$((PASS+1))
    else
        echo "  ❌ zkvm/$pkg FAILED"; FAIL=$((FAIL+1))
    fi
done

# ─── SUMMARY ───────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  RESULTS: ✅ $PASS passed  |  ❌ $FAIL failed"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
[ $FAIL -eq 0 ] && echo " 🏆 ALL TESTS PASSED — Ready to submit!" || { echo " 🛑 Fix failures before submitting."; exit 1; }
echo ""
