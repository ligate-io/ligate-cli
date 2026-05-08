# Changelog

All notable changes to `ligate-cli`. Pre-launch; everything sits
under `[Unreleased]` until the first tagged release alongside
`ligate-devnet-1` going live.

Format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- `ligate info` subcommand. One-line operator check after a deploy: prints `chain_id`, `chain_hash`, and node `version` for the configured RPC. Pure HTTP GET against `/v1/rollup/info`; no signing, no keystore touched. Supports `--json` for pipelining (`export LIGATE_CHAIN_HASH=$(ligate info --json | jq -r .chain_hash)`). Useful first command in the post-`ligate-node-up` smoke test from `docs/development/public-devnet-deploy.md`. Closes the gap where operators had to `curl https://rpc.ligate.io/v1/rollup/info | jq` instead of having a first-party flag.
- `.github/workflows/release.yml` — tagged-release workflow that
  cross-compiles `ligate` for the four target platforms operators
  and developers run on (linux x86_64 / arm64, darwin arm64 / amd64),
  packages each as a `.tar.gz` with SHA-256 checksum, and attaches
  them to a GitHub Release with the `## [Unreleased]` section of this
  CHANGELOG as release notes. Triggered on `v*` tag pushes;
  `workflow_dispatch` runs the build matrix as a dry-run without
  publishing. Mirrors `ligate-chain` and `ligate-io/faucet` release
  workflows exactly so binaries across the three repos share an
  install pattern (`wget` of the tarball; `cargo install --git` as
  fallback). Drops the "compile Rust on the laptop / VM for ~20
  minutes" step from the operator + builder install flow.

### Initial scaffold

- `ligate keys generate | list | show` for local Ed25519 keystore
  management. On-disk format byte-compatible with the chain's
  `ligate-genesis-tool keys generate`.
- `ligate balance` for read-only `$LGT` balance queries against a
  running node.
- `ligate transfer` for `bank.transfer` build-sign-submit using
  `ligate-client::submit::Submitter`.
- `ligate faucet` for one-shot drips against a deployed faucet
  service.
- Shared chain-identity flag plumbing (`--chain-id`, `--chain-hash`,
  `--token-id`) with env-var fallbacks (`LIGATE_CHAIN_ID`,
  `LIGATE_CHAIN_HASH`, `LIGATE_LGT_TOKEN_ID`).
- `--rpc URL` and `--json` global flags.
- CI: fmt + clippy + check on Ubuntu, libclang installed for
  librocksdb-sys, `SKIP_GUEST_BUILD=1` + `RISC0_SKIP_BUILD_KERNELS=1`
  + `CONSTANTS_MANIFEST_PATH` to keep the chain's risc0 prover dep
  from blocking the build.
- `cargo test` CI job is intentionally commented out pending risc0
  toolchain cleanup in `ligate-rollup`.
- Tracking: [`ligate-chain#112`](https://github.com/ligate-io/ligate-chain/issues/112).

### Out of scope for v0

Subcommands deferred until their chain-side modules ship:

- `ligate attest submit | verify` (Themisra attestation module)
- `ligate schema register | show` (schema registry module)
- `ligate attestor-set register | show` (attestor-set module)
- `ligate node start` (operator wrapper around `cargo run --bin ligate-node`)
