# Changelog

All notable changes to `ligate-cli`. First tagged release is
`v0.1.0-devnet`, cut alongside `ligate-chain` `v0.1.0-devnet` and
`ligate-devnet-1` going live.

Format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0-devnet] - 2026-05-15

First tagged release. Cut alongside `ligate-chain` `v0.1.0-devnet`
and the `ligate-devnet-1` public devnet rung. Bundles the rc.1
scaffold plus every operator + builder subcommand merged since.

### Added

- `ligate info` subcommand. One-line operator check after a deploy: prints `chain_id`, `chain_hash`, and node `version` for the configured RPC. Pure HTTP GET against `/v1/rollup/info`; no signing, no keystore touched. Supports `--json` for pipelining (`export LIGATE_CHAIN_HASH=$(ligate info --json | jq -r .chain_hash)`). Useful first command in the post-`ligate-node-up` smoke test from `docs/development/public-devnet-deploy.md`. Closes the gap where operators had to `curl https://rpc.ligate.io/v1/rollup/info | jq` instead of having a first-party flag. (#11)
- `ligate completions <SHELL>` subcommand. Prints a clap-generated completion script to stdout for `bash`, `zsh`, `fish`, `powershell`, or `elvish`; pipe to the install path for your shell and tab completion works at the subcommand, sub-subcommand, and flag level. Closes #2. (#13)
- `ligate transfer --print-tx-bytes`. Builds + signs the transfer locally and emits the borsh-encoded `RuntimeCall` bytes (hex) instead of submitting. Lets the cli act as an offline signer for the typescript SDK's submit path; pairs with the `ligate-js` `submitRawTx` flow added in [`ligate-js#18`](https://github.com/ligate-io/ligate-js/pull/18). (#17)
- Attestation subcommands. Wires the Themisra v0 surface against `ligate-chain`'s schema-registry, attestor-set, and attestation modules:
  - `ligate register-attestor-set` — registers a quorum of attestor public keys plus an M-of-N threshold; returns the `las1...` id.
  - `ligate register-schema` — registers an attestation schema from a JSON definition file; returns the `lsc1...` id.
  - `ligate submit-attestation` — submits a threshold-signed attestation under an existing schema.
  - `ligate query schema | attestor-set | attestation` — read-only fetch by id; no signing or keystore touch.
  Closes the "remaining v0 surface" gap from the rc.1 README; only `node start` deferred. (#20)
- `.github/workflows/release.yml` — tagged-release workflow that cross-compiles `ligate` for the four target platforms operators and developers run on (linux x86_64 / arm64, darwin arm64 / amd64), packages each as a `.tar.gz` with SHA-256 checksum, and attaches them to a GitHub Release with the `## [Unreleased]` section of this CHANGELOG as release notes. Triggered on `v*` tag pushes; `workflow_dispatch` runs the build matrix as a dry-run without publishing. Mirrors `ligate-chain` and `ligate-io/faucet` release workflows exactly so binaries across the three repos share an install pattern (`wget` of the tarball; `cargo install --git` as fallback). Drops the "compile Rust on the laptop / VM for ~20 minutes" step from the operator + builder install flow. (#10)

### Changed

- Bech32m chain identity rewrite. Address / schema / attestor-set / attestation parsers now accept the new `lig1` / `lsc1` / `las1` / `lat1` HRPs and the `token_1...` token-id form introduced by `ligate-chain` commit `0ac7e5b`; SDK fork `[patch]` table added so the cli pins the same chain-runtime revision as the node. (#12)
- Chain dependency pin bumped to current `ligate-chain` `main` (Sovereign SDK fork rev advanced in lockstep). Realigns the cli against the chain runtime as devnet preparation lands. (#18)
- Install docs (`README.md`) split into two distinct paths in preference order: pre-built tarball (recommended, ~30s end-to-end) and `cargo install --git` (fallback, ~10–15 min cold-cache). Documents `SKIP_GUEST_BUILD=1` + `RISC0_SKIP_BUILD_KERNELS=1` env vars required for source builds to skip the risc0 guest compile. Closes #3. (#14)

### Chore

- CLA Assistant Lite workflow + canonical `CLA.md` (mirrors `ligate-chain#257`). `sstefdev` allowlisted as an org member rather than a contributor. (#15, #16)

### Initial scaffold (`v0.0.1-rc.1`, never released)

The rc.1 tag was pushed on 2026-05-08 but its Release workflow was
manually cancelled mid-build; no GitHub Release artifact was ever
produced. Folded into `0.1.0-devnet` for completeness.

- `ligate keys generate | list | show` for local Ed25519 keystore management. On-disk format byte-compatible with the chain's `ligate-genesis-tool keys generate`.
- `ligate balance` for read-only `$LGT` balance queries against a running node.
- `ligate transfer` for `bank.transfer` build-sign-submit using `ligate-client::submit::Submitter`.
- `ligate faucet` for one-shot drips against a deployed faucet service.
- Shared chain-identity flag plumbing (`--chain-id`, `--chain-hash`, `--token-id`) with env-var fallbacks (`LIGATE_CHAIN_ID`, `LIGATE_CHAIN_HASH`, `LIGATE_LGT_TOKEN_ID`).
- `--rpc URL` and `--json` global flags.
- CI: fmt + clippy + check on Ubuntu, libclang installed for librocksdb-sys, `SKIP_GUEST_BUILD=1` + `RISC0_SKIP_BUILD_KERNELS=1` + `CONSTANTS_MANIFEST_PATH` to keep the chain's risc0 prover dep from blocking the build.
- `cargo test` CI job is intentionally commented out pending risc0 toolchain cleanup in `ligate-rollup`.
- Tracking: [`ligate-chain#112`](https://github.com/ligate-io/ligate-chain/issues/112).

### Deferred

Subcommand still gated on chain-side work:

- `ligate node start` (operator wrapper around `cargo run --bin ligate-node`).
