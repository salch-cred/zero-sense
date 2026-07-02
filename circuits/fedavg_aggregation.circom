pragma circom 2.1.6;

/*
 * ZeroSense — zk-FedAvg Fleet Learning circuit (design spec)
 * ------------------------------------------------------------------
 * Statement proved (Groth16 over BN254, matching Stellar's native
 * CAP-0074 BN254 host functions so the proof verifies cheaply on-chain via
 * contracts/bn254_verifier — the same generic verifier that also backs
 * circuits/fleet_membership.circom):
 *
 *   Given N committed local model updates c_1..c_N (Poseidon commitments
 *   already recorded on-chain by contracts/fleet_learning::submit_local_update,
 *   in submission order), the prover (the fleet's aggregation server) knows
 *   the N underlying fixed-point weight-delta vectors w_1..w_N and
 *   per-client sample counts n_1..n_N such that:
 *
 *     1. commitment_i == Poseidon(w_i (flattened), n_i, salt_i)   for i in 1..N
 *     2. commitments_root == fold_left(Poseidon, c_1..c_N)
 *        i.e. root = Poseidon(...Poseidon(Poseidon(c_1, c_2), c_3)..., c_N)
 *        (matches contracts/fleet_learning::compute_commitments_root exactly
 *        — same left-fold order — so the contract can recompute the public
 *        signal independently and the proof cannot be about a different set
 *        of contributions)
 *     3. aggregated[j] == floor( sum_i(w_i[j] * n_i) / sum_i(n_i) )
 *        for every weight coordinate j — fixed-point FedAvg, integer
 *        arithmetic (see Xu et al. 2023, arXiv:2312.04579, "An efficient and
 *        privacy-preserving decentralized federated learning algorithm",
 *        for the same weighted-average-in-ZK constraint this circuit follows)
 *     4. aggregated_hash == Poseidon(aggregated[0], ..., aggregated[D-1])
 *
 *   Public inputs:  [round, commitments_root, aggregated_hash]
 *   Private inputs: w_1..w_N, n_1..n_N, salt_1..salt_N
 *
 * Research grounding
 * -------------------
 *   - Jin et al. 2025, "Zero-Knowledge Federated Learning: A New Trustworthy
 *     and Privacy-Preserving Distributed Learning Paradigm" (arXiv:2503.15550)
 *   - Xu et al. 2023, "An efficient and privacy-preserving decentralized
 *     federated learning algorithm" (arXiv:2312.04579) — proves FedAvg
 *     correctness in ZK; the template this circuit's averaging constraint
 *     follows.
 *   - Bruschi, Esposito, Gagliardoni, Rizzini 2025, "SoK: Verifiable
 *     Federated Learning" (IACR ePrint 2025/2296) — survey confirming that
 *     ZK-proved *aggregation* correctness (vs. only proving local training)
 *     is the less-explored, harder half of verifiable FL.
 *   - Pacheco et al., DARS 2024, "Securing Federated Learning in Robot
 *     Swarms using Blockchain" — blockchain-token-gated FL for *robot
 *     swarms specifically*, but reputation-token based, not ZK-proved
 *     aggregation correctness.
 *
 * Novelty check (see README.md "Why This Is Novel" for the full writeup)
 * ------------------------------------------------------------------------
 * Checked against the above papers plus GitHub prior art (Veriblock-FL,
 * fl-chain-data-sharing, ZKML-Soroban, openzktool, soroban-verifier-gen):
 * every existing ZK-FL implementation we found targets Ethereum/generic
 * blockchains and generic clients, not a physical robot fleet with
 * on-chain identity and autonomous per-round payment. Combining
 * (a) ZK-proved FedAvg aggregation correctness, (b) Poseidon commitments
 * bound to fleet_identity-style robot accounting, and (c) autonomous
 * Stellar reward payout gated on that proof is, as far as our research
 * could determine, unbuilt elsewhere.
 *
 * STATUS
 * ------
 * The on-chain consumer (contracts/fleet_learning) is complete, wired, and
 * tested. The circuit below is a fully-specified, annotated design showing
 * the exact constraint system; compiling it with circomlib's Poseidon
 * template (parameterized to match soroban-poseidon's BN254 sponge),
 * running a real trusted setup, and wiring a live Gnark/snarkjs prover is
 * the remaining step before finalize_round can be called with a genuine
 * proof. The contract-side verification path is already real — see
 * contracts/bn254_verifier's Gnark-fixture tests for a proof of that exact
 * path working end-to-end (with a different circuit's real proof).
 */

template FedAvgAggregation(N, D, BITS) {
    // N = number of contributing robots this round
    // D = flattened, quantized model-delta length
    // BITS = fixed-point width used for the range-checked integer division

    signal input round;
    signal input commitments_root;
    signal input aggregated_hash;

    signal input w[N][D];   // private: each robot's fixed-point weight delta
    signal input n[N];      // private: each robot's local sample count
    signal input salt[N];   // private: per-robot commitment salt

    // --- 1. Recompute each commitment and left-fold the root ---
    // commitment[i] <== Poseidon(w[i] (flattened), n[i], salt[i]);
    // root          <== fold_left(Poseidon, commitment[0..N-1]);
    // root === commitments_root;
    //
    // (Poseidon component wiring omitted here — use circomlib's Poseidon
    // template, instantiated with the same BN254 round constants as
    // soroban-poseidon, so the in-circuit hash and the on-chain
    // `compute_commitments_root` host-function hash agree bit-for-bit.)

    // --- 2. FedAvg with integer division, range-checked ---
    // totalN        <== sum(n[i])
    // weighted[j]   <== sum_i(w[i][j] * n[i])
    // aggregated[j] <== floor(weighted[j] / totalN), constrained via the
    //                   standard Circom quotient+remainder trick:
    //                     weighted[j] === aggregated[j] * totalN + remainder[j]
    //                     0 <= remainder[j] < totalN
    //                   (remainder range-checked with BITS-width comparators,
    //                   totalN > 0 range-checked separately)

    // --- 3. Bind the aggregated result to the public aggregated_hash ---
    // computedHash <== Poseidon(aggregated[0..D-1]);
    // computedHash === aggregated_hash;
}

component main {public [round, commitments_root, aggregated_hash]} =
    FedAvgAggregation(8, 256, 16); // example sizing: 8 robots/round, 256-dim delta, Q16 fixed point
