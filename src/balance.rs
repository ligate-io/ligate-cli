//! `ligate balance` subcommand: read the `$LGT` balance of an address.
//!
//! Pure read path: queries the node's REST endpoint, no signing. Uses
//! [`sov_node_client::NodeClient::get_balance_for_holder`] which takes
//! the address as a `&str` and avoids the typed-Spec hop.

use anyhow::{Context, Result};
use clap::Args;
use ligate_client::submit::Submitter;
use ligate_rollup::MockRollupSpec;
use serde::Serialize;
use sov_bank::TokenId;
use sov_modules_api::execution_mode::Native;

use crate::cli::GlobalArgs;
use crate::config::parse_token_id;

/// Spec carries the address shape; for read queries the DA flavour
/// doesn't matter (mirrors transfer.rs).
type S = MockRollupSpec<Native>;

#[derive(Debug, Args)]
pub struct BalanceCmd {
    /// `lig1...` address to query.
    pub address: String,

    /// Token id (64-char hex). Defaults to `$LGT` once that's a
    /// well-known constant; for now the operator passes it explicitly.
    /// Pull from `bank.json`'s `token_id` at chain genesis.
    #[arg(long, env = "LIGATE_LGT_TOKEN_ID")]
    pub token_id: String,
}

#[derive(Serialize)]
struct BalanceJson<'a> {
    address: &'a str,
    token_id: String,
    amount_nano: u128,
    amount_lgt: f64,
}

impl BalanceCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        let token_id: TokenId = parse_token_id(&self.token_id)?;
        let rpc = global.rpc_with_v1();
        let submitter = Submitter::new(&rpc)
            .await
            .with_context(|| format!("connecting to {rpc}"))?;

        let amount = submitter
            .inner()
            .get_balance_for_holder::<S>(&self.address, &token_id)
            .await
            .with_context(|| format!("querying balance for {}", self.address))?;

        // SDK's Amount is a newtype around u128. Pull the inner.
        let nano: u128 = amount.0;
        let lgt = (nano as f64) / 1_000_000_000.0;

        if global.json {
            let payload = BalanceJson {
                address: &self.address,
                token_id: hex::encode(token_id.as_bytes()),
                amount_nano: nano,
                amount_lgt: lgt,
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("{}: {lgt:.9} $LGT ({nano} nano)", self.address);
        }
        Ok(())
    }
}
