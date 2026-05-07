//! `ligate faucet` subcommand: claim a one-shot drip from the public
//! devnet faucet.
//!
//! Talks to the `ligate-io/faucet` HTTP service over its `POST /faucet`
//! endpoint. Useful as a one-line bootstrap for new users who just
//! generated a key and need `$LGT` to pay fees on their first
//! transaction.
//!
//! No keystore involvement: the faucet drips to the address regardless
//! of whether the requester holds the private key. The CLI just
//! wraps the curl pattern documented in the faucet README.

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};

use crate::cli::GlobalArgs;

/// Default faucet base URL. Switch via `--faucet-url` for a self-hosted
/// instance.
const DEFAULT_FAUCET_URL: &str = "https://faucet.ligate.io";

#[derive(Debug, Args)]
pub struct FaucetCmd {
    /// `lig1...` recipient address.
    pub address: String,

    /// Override the faucet base URL.
    #[arg(long, env = "LIGATE_FAUCET_URL", default_value = DEFAULT_FAUCET_URL)]
    pub faucet_url: String,
}

#[derive(Serialize)]
struct FaucetReq<'a> {
    address: &'a str,
}

#[derive(Deserialize)]
struct FaucetSuccess {
    address: String,
    tx_hash: String,
    amount_nano: u128,
    drip_amount_lgt: f64,
}

#[derive(Deserialize)]
struct FaucetError {
    error: String,
    retry_after_secs: Option<u64>,
}

impl FaucetCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        let url = format!("{}/faucet", self.faucet_url.trim_end_matches('/'));

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(&FaucetReq {
                address: &self.address,
            })
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;

        let status = resp.status();
        let body_bytes = resp.bytes().await.context("reading faucet response body")?;

        if status.is_success() {
            let s: FaucetSuccess = serde_json::from_slice(&body_bytes).with_context(|| {
                format!(
                    "decoding faucet success body: {}",
                    String::from_utf8_lossy(&body_bytes)
                )
            })?;
            if global.json {
                println!("{}", String::from_utf8_lossy(&body_bytes));
            } else {
                println!("Drip accepted by faucet:");
                println!("  address: {}", s.address);
                println!(
                    "  amount:  {} $LGT ({} nano)",
                    s.drip_amount_lgt, s.amount_nano
                );
                println!("  tx:      {}", s.tx_hash);
            }
            Ok(())
        } else {
            // Faucet error responses use a stable JSON shape; fall
            // back to raw body if the shape doesn't match.
            match serde_json::from_slice::<FaucetError>(&body_bytes) {
                Ok(e) => {
                    let retry = e
                        .retry_after_secs
                        .map(|s| format!(" (retry in {s}s)"))
                        .unwrap_or_default();
                    anyhow::bail!("faucet returned {status}: {}{retry}", e.error);
                }
                Err(_) => {
                    anyhow::bail!(
                        "faucet returned {status}: {}",
                        String::from_utf8_lossy(&body_bytes)
                    );
                }
            }
        }
    }
}
