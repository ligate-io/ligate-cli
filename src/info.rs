//! `ligate info` subcommand: print chain identity for the configured RPC.
//!
//! First-line operator check after a deploy ("am I talking to
//! `ligate-devnet-1`? what's the canonical `chain_hash`?"). Cheap:
//! one HTTP GET, no signing, no keystore touched. Useful in shell
//! pipelines to extract the `chain_hash` for env-var hydration:
//!
//! ```sh
//! export LIGATE_CHAIN_HASH=$(ligate info --json | jq -r .chain_hash)
//! ```
//!
//! Since `ligate-chain@0ac7e5b` the `chain_hash` field comes back as a
//! bech32m `lsch1...` string (older chain revs emitted bare hex). The
//! string is passed verbatim into `ligate transfer --chain-hash` /
//! `LIGATE_CHAIN_HASH`; [`crate::config::parse_chain_hash`] accepts both
//! forms, so the env-var pipeline keeps working across the bump.

use anyhow::{Context, Result};
use clap::Args;
use ligate_client::submit::Submitter;
use serde::{Deserialize, Serialize};

use crate::cli::GlobalArgs;

#[derive(Debug, Args)]
pub struct InfoCmd {}

/// Shape of `GET /v1/rollup/info`. Mirrors the chain's
/// `crates/rollup/src/info.rs::RollupInfo` so a schema change there
/// breaks deserialisation here loudly.
#[derive(Serialize, Deserialize)]
struct RollupInfo {
    chain_id: String,
    chain_hash: String,
    #[serde(default)]
    version: String,
}

impl InfoCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        let rpc = global.rpc_with_v1();
        // For error messages only -- the actual fetch passes a path to
        // `http_get` (see below).
        let full_url = format!("{rpc}/rollup/info");

        // Use the SDK's NodeClient (via `Submitter`) for the GET so the
        // URL-shaping + transport behaviour stays consistent with
        // `balance` / `transfer` / `faucet`. `new_unchecked` skips the
        // schema probe (we WANT to ask `info` even if the chain just
        // booted and isn't fully ready), so the existing `Submitter::new`
        // path isn't right here.
        //
        // IMPORTANT: `NodeClient::http_get` prepends its own `base_url`
        // (set from the `&rpc` passed to `Submitter::new_unchecked`).
        // So we pass the PATH `/rollup/info`, not the full URL. Passing
        // the full URL produces a doubled string like
        // `https://rpc.../v1https://rpc.../v1/rollup/info`, which
        // `reqwest` issues against the host portion it can parse out
        // (`rpc.ligate.io`); the chain then returns 404 with an empty
        // body, and `http_get` happily returns `Ok("")` because it
        // doesn't check the status code. See SDK
        // `crates/utils/sov-node-client/src/lib.rs::http_get`.
        let submitter = Submitter::new_unchecked(&rpc);
        let body = submitter
            .inner()
            .http_get("/rollup/info")
            .await
            .with_context(|| format!("GET {full_url}"))?;
        let info: RollupInfo = serde_json::from_str(&body).with_context(|| {
            format!("parsing /rollup/info JSON from {full_url}: {body}")
        })?;

        if global.json {
            // Pretty-print so the operator can pipe to `jq`.
            println!("{}", serde_json::to_string_pretty(&info)?);
        } else {
            println!("RPC:        {rpc}");
            println!("chain_id:   {}", info.chain_id);
            println!("chain_hash: {}", info.chain_hash);
            if !info.version.is_empty() {
                println!("version:    {}", info.version);
            }
        }
        Ok(())
    }
}
