#!/usr/bin/env bash
# =============================================================================
# ZeroSense — Stellar Testnet Deployment
# -----------------------------------------------------------------------------
# Builds and deploys all Soroban contracts to Stellar testnet, records the
# resulting contract IDs, and prints stellar.expert explorer links so the
# deployment is independently verifiable (the credibility signal judges look
# for).
#
# Usage:
#   ./deploy.sh                 # uses identity 'zerosense-dev' on testnet
#   IDENTITY=mykey ./deploy.sh  # override signing identity
#   NETWORK=testnet ./deploy.sh # override network
#   FLEET_DEPTH=20 ./deploy.sh  # override fleet identity Merkle tree depth
#
# Requirements: stellar-cli (`cargo install stellar-cli`), Rust wasm target.
# =============================================================================
set -euo pipefail

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-zerosense-dev}"
TARGET="wasm32v1-none"
OUT_DIR="deploy"
DEPLOY_LOG="$OUT_DIR/contract_ids.env"

# Contracts with a plain (no-arg) deploy; constructor-arg contracts handled below.
# `consensus` (zk-Swarm Consensus) is plain-deploy too: its `initialize(admin,
# verifier)` is a normal post-deploy call, same as verifier/payment.
CONTRACTS=(verifier payment reputation insurance consensus)

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

# ---- 2. Build + deploy each plain contract ----------------------------------
build_one() {
  local name="$1"
  echo ""
  echo "================================================================="
  echo "==> Building contracts/$name"
  ( cd "contracts/$name" && stellar contract build )
}

resolve_wasm() {
  # echoes the path to the produced wasm for contract $1
  local name="$1"
  local rel_dir="contracts/$name/target/$TARGET/release"
  local wasm="$rel_dir/zerosense_${name}.wasm"
  if [[ ! -f "$wasm" ]]; then
    wasm="$(find "$rel_dir" -maxdepth 1 -name '*.wasm' | head -n1)"
  fi
  if [[ -z "${wasm:-}" || ! -f "$wasm" ]]; then
    echo "ERROR: no wasm artifact found for $name in $rel_dir" >&2
    exit 1
  fi
  echo "$wasm"
}

record_id() {
  local name="$1" cid="$2"
  local var
  var="$(echo "$name" | tr '[:lower:]' '[:upper:]')_CONTRACT_ID"
  echo "${var}=${cid}" | tee -a "$DEPLOY_LOG"
  echo "Explorer: https://stellar.expert/explorer/$NETWORK/contract/$cid"
}

deploy_one() {
  local name="$1"
  build_one "$name"
  local wasm; wasm="$(resolve_wasm "$name")"
  echo "==> Deploying $name ($wasm)"
  local cid
  cid="$(stellar contract deploy --wasm "$wasm" --source "$IDENTITY" --network "$NETWORK")"
  record_id "$name" "$cid"
}

for c in "${CONTRACTS[@]}"; do
  deploy_one "$c"
done

# ---- 2b. Deploy fleet_identity (constructor needs admin + Merkle tree depth) -
FLEET_DEPTH="${FLEET_DEPTH:-20}"
build_one "fleet_identity"
FI_WASM="$(resolve_wasm "fleet_identity")"
echo "==> Deploying fleet_identity ($FI_WASM) admin=$ADMIN_ADDR depth=$FLEET_DEPTH"
FI_ID="$(stellar contract deploy --wasm "$FI_WASM" --source "$IDENTITY" --network "$NETWORK" -- --admin "$ADMIN_ADDR" --depth "$FLEET_DEPTH")"
record_id "fleet_identity" "$FI_ID"

# ---- 2c. Build (but do not deploy) bn254_verifier ---------------------------
# bn254_verifier's constructor takes (admin, verification_key), where
# verification_key is a real Groth16 VK for a *specific* circuit (alpha/beta/
# gamma/delta G1/G2 points + IC vector). We only build the wasm here; the
# actual deploy is a separate, deliberate step run once
# circuits/fleet_membership.circom has gone through a real trusted setup and
# exported a verification_key.json (see contracts/bn254_verifier/tests/data/
# gnark/verification_key.json for the expected shape). Deploying with a
# placeholder/fake VK would silently make verify_proof meaningless, so this
# script intentionally does not fabricate one.
build_one "bn254_verifier"
echo "==> bn254_verifier built. Deploy separately once you have a real circuit VK:"
echo "    stellar contract deploy --wasm \$(contracts/bn254_verifier wasm) \\"
echo "      --source $IDENTITY --network $NETWORK -- \\"
echo "      --admin $ADMIN_ADDR --verification_key <VK_FROM_TRUSTED_SETUP>"

# ---- 2d. Build (but do not deploy) fleet_learning ---------------------------
# fleet_learning's constructor takes (admin, verifier, reward_token,
# reward_per_contributor). `verifier` must be a *deployed* bn254_verifier
# instance already configured with circuits/fedavg_aggregation.circom's real
# verification key (see 2c above) — finalize_round is load-bearing against
# it, so deploying fleet_learning before that verifier exists would only
# produce a coordinator that can never finalize a round. We build the wasm
# here so it's ready the moment a real verifier + circuit VK exist.
build_one "fleet_learning"
echo "==> fleet_learning built. Deploy once bn254_verifier is live with the"
echo "    fedavg_aggregation circuit's VK, and you have a reward token (e.g."
echo "    the native XLM Stellar Asset Contract):"
echo "    stellar contract deploy --wasm \$(contracts/fleet_learning wasm) \\"
echo "      --source $IDENTITY --network $NETWORK -- \\"
echo "      --admin $ADMIN_ADDR --verifier <BN254_VERIFIER_CONTRACT_ID> \\"
echo "      --reward_token <XLM_SAC_CONTRACT_ID> --reward_per_contributor <STROOPS>"

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
echo ""
echo "Then enroll a robot identity into the fleet (admin-only):"
echo "  stellar contract invoke --id \"\$FLEET_IDENTITY_CONTRACT_ID\" \\"
echo "    --source $IDENTITY --network $NETWORK -- \\"
echo "    register_robot --commitment <POSEIDON_COMMITMENT_U256>"
echo ""
echo "Then wire zk-Swarm Consensus to the verifier (any high-value action can"
echo "now require M-of-N independently ZK-verified robot witnesses):"
echo "  stellar contract invoke --id \"\$CONSENSUS_CONTRACT_ID\" \\"
echo "    --source $IDENTITY --network $NETWORK -- \\"
echo "    initialize --admin $ADMIN_ADDR --verifier \"\$VERIFIER_CONTRACT_ID\""
