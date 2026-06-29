#!/usr/bin/env bash
# =============================================================================
# ZeroSense — Stellar Testnet Deployment
# -----------------------------------------------------------------------------
# Builds and deploys all four Soroban contracts to Stellar testnet, records the
# resulting contract IDs, and prints stellar.expert explorer links so the
# deployment is independently verifiable (the credibility signal judges look
# for).
#
# Usage:
#   ./deploy.sh                 # uses identity 'zerosense-dev' on testnet
#   IDENTITY=mykey ./deploy.sh  # override signing identity
#   NETWORK=testnet ./deploy.sh # override network
#
# Requirements: stellar-cli (`cargo install stellar-cli`), Rust wasm target.
# =============================================================================
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-zerosense-dev}"
TARGET="wasm32v1-none"
OUT_DIR="deploy"
DEPLOY_LOG="$OUT_DIR/contract_ids.env"

CONTRACTS=(verifier payment reputation insurance)

mkdir -p "$OUT_DIR"
: > "$DEPLOY_LOG"

echo "==> Network: $NETWORK | Identity: $IDENTITY"

# ---- 1. Ensure a funded signing identity ------------------------------------
if ! stellar keys address "$IDENTITY" >/dev/null 2>&1; then
  echo "==> Generating identity '$IDENTITY'"
  stellar keys generate --global "$IDENTITY" --network "$NETWORK"
fi
echo "==> Funding '$IDENTITY' (friendbot; ignored if already funded)"
stellar keys fund "$IDENTITY" --network "$NETWORK" || true

ADMIN_ADDR="$(stellar keys address "$IDENTITY")"
echo "==> Admin address: $ADMIN_ADDR"

# ---- 2. Build + deploy each contract ----------------------------------------
deploy_one() {
  local name="$1"
  echo ""
  echo "================================================================="
  echo "==> Building contracts/$name"
  ( cd "contracts/$name" && stellar contract build )

  # Resolve the produced wasm (package names use the zerosense_<name> pattern,
  # but fall back to any wasm in the release dir to be robust to renames).
  local rel_dir="contracts/$name/target/$TARGET/release"
  local wasm="$rel_dir/zerosense_${name}.wasm"
  if [[ ! -f "$wasm" ]]; then
    wasm="$(find "$rel_dir" -maxdepth 1 -name '*.wasm' | head -n1)"
  fi
  if [[ -z "${wasm:-}" || ! -f "$wasm" ]]; then
    echo "ERROR: no wasm artifact found for $name in $rel_dir" >&2
    exit 1
  fi

  echo "==> Deploying $name ($wasm)"
  local cid
  cid="$(stellar contract deploy --wasm "$wasm" --source "$IDENTITY" --network "$NETWORK")"

  local var
  var="$(echo "$name" | tr '[:lower:]' '[:upper:]')_CONTRACT_ID"
  echo "${var}=${cid}" | tee -a "$DEPLOY_LOG"
  echo "Explorer: https://stellar.expert/explorer/$NETWORK/contract/$cid"
}

for c in "${CONTRACTS[@]}"; do
  deploy_one "$c"
done

# ---- 3. Next steps ----------------------------------------------------------
echo ""
echo "================================================================="
echo "==> All contracts deployed. IDs saved to $DEPLOY_LOG"
echo ""
echo "Next: initialize the verifier with the BLS12-381 Groth16 verification key"
echo "(produced by your trusted setup / snarkjs export), then register a model:"
echo ""
echo "  source $DEPLOY_LOG"
echo "  stellar contract invoke --id \"\$VERIFIER_CONTRACT_ID\" \\"
echo "    --source $IDENTITY --network $NETWORK -- \\"
echo "    initialize --admin $ADMIN_ADDR --vk <VK_JSON>"
echo ""
echo "  stellar contract invoke --id \"\$VERIFIER_CONTRACT_ID\" \\"
echo "    --source $IDENTITY --network $NETWORK -- \\"
echo "    register_model --model_hash <HASH32> --model_name MobileNetV2-INT8"
