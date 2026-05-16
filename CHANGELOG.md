# Changelog

All notable changes to `ligate-cli`. First tagged release is
`v0.1.0-devnet`, cut alongside `ligate-chain` `v0.1.0-devnet` and
`ligate-devnet-1` going live.

Format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.2-devnet] - 2026-05-16

Builder-side release. Adds the attestor half of the attestation flow (`sign-attestation`), `keys show --pubkey` for first-party `lpk1...` derivation, and consolidates the nonce-path workaround across `transfer` + attestation verbs onto a single shared helper. Also bumps the SDK fork pin to `eab3f9d0` to align with the chain repo (the upstream `NodeClient::get_nonce_for_public_key` uniqueness-path fix landed; local workaround stays in `src/nonce.rs` until every chain rev the CLI talks to includes the fix).

### Added

- `ligate sign-attestation` subcommand. Builds the canonical SHA-256(borsh(SignedAttestationPayload)) digest via `ligate_client::attestation_digest`, signs with the named keystore role, and prints the entry shape that `submit-attestation --signatures` consumes. Closes the gap where attestor operators had to either use the Rust `ligate-client` crate directly or roll their own ed25519+borsh outside the CLI. Pairs with the matching JS-side helpers in `ligate-js` `v0.1.1-devnet`. (#28)
- `--payload-file` flag on `sign-attestation`. SHA-256s a canonical payload file as a convenience so attestors don't need a separate hashing step before signing. (#28)
- `ligate keys show <role> --pubkey`. Prints the `lpk1...` bech32m public key for the given keystore role. Operators registering an attestor set now have a first-party path from "keystore on disk" to "membership entry"; previously had to hand-roll the bech32m encoding. (#28)
- `ligate keys generate` now echoes the `lpk1...` pubkey alongside the address. Same rationale: keep the bech32m derivation off the operator's plate. `GeneratedKey` gains a `pubkey` field for `--json`-consuming callers. (#28)
- `examples/derive_digest.rs`. Standalone binary that prints the borsh bytes + digest for the canonical LIP-5 test vector. Lets non-Rust attestor implementations cross-check their own digest computation byte-for-byte against the reference. (#28)
- `.pre-commit-config.yaml`. Local `cargo fmt --check` gate at commit time, matching the chain repo + api repo patterns. One-time setup: `pre-commit install`. (#30)

### Changed

- Nonce-path workaround consolidated. Both `src/transfer.rs` and the attestation verbs (`register-attestor-set`, `register-schema`, `submit-attestation`) now share `src/nonce.rs::fetch_account_nonce`, which hits the chain's `/modules/uniqueness/...` path directly. Previously only the attestation verbs had the workaround; `transfer` against an account with on-chain nonce > 0 silently sent a tx with nonce 0 and got `Tx bad nonce` from the chain. Tracked upstream in `ligate-io/sovereign-sdk#2`; this helper becomes deletable when every chain rev the CLI talks to includes the upstream fix. (#28)
- SDK fork pin bumped to `ligate-io/sovereign-sdk@eab3f9d0` (was `49e9b2057`). Picks up the upstream uniqueness-path fix. Stays at the same upstream `Sovereign-Labs/sovereign-sdk` head, just realigns the [patch] table with the chain repo. (#29)

## [0.1.1-devnet] - 2026-05-15

Patch release. Fixes two URL-doubling bugs in the SDK-mediated HTTP path that surfaced during the `ligate-devnet-1` first-deploy smoke. Same root cause across both:

### Fixed

- `ligate info` failed against the live devnet with `parsing /rollup/info JSON: EOF while parsing a value at line 1 column 0` despite the chain serving the expected JSON. Root cause: `Submitter::inner().http_get(full_url)` triggered the SDK's `NodeClient::http_get` which prepends its own `base_url`, producing a doubled URL (`<base>/rollup/info<base>/rollup/info`) that 404s with an empty body. `http_get` then returned `Ok("")` because the SDK doesn't check status codes. Fix: pass the PATH `/rollup/info` (not the full URL). (#24)
- `ligate transfer` and `ligate <attestation cmd>` `wait_for_inclusion` polling returned "tx included" on the first poll regardless of whether the tx had actually landed -- same root cause as the `info` bug. The `.is_ok()` check in the poll loop matched `Ok("")` returned by `http_get` for the chain's 404-on-not-yet-indexed response. The tx itself still landed within ~12s (Mocha's block time) but the cli's claim about inclusion was vacuous. Fix: pass the PATH `/ledger/txs/{hash}` and additionally check that the response body is non-empty (empty body = not yet indexed = keep polling; populated body = chain returning the tx JSON = return). (#25)



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
