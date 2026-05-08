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
        let url = format!("{rpc}/rollup/info");

        // Use the SDK's NodeClient (via `Submitter`) for the GET so the
        // URL-shaping + transport behaviour stays consistent with
        // `balance` / `transfer` / `faucet`. `new_unchecked` skips the
        // schema probe (we WANT to ask `info` even if the chain just
        // booted and isn't fully ready), so the existing `Submitter::new`
        // path isn't right here.
        let submitter = Submitter::new_unchecked(&rpc);
        let body = submitter
            .inner()
            .http_get(&url)
            .await
            .with_context(|| format!("GET {url}"))?;
        let info: RollupInfo = serde_json::from_str(&body)
            .with_context(|| format!("parsing /rollup/info JSON: {body}"))?;

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
