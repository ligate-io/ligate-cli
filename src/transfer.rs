//! `ligate transfer` subcommand: send `$LGT` from a local key to
//! another address.
//!
//! Mirrors the faucet's `signer.rs` build-sign-submit pipeline. The
//! difference is the source of the signing key (local keystore vs
//! env-var hot key), the source of the nonce (refetched from chain
//! per call vs in-memory atomic counter), and the call site (clap
//! subcommand vs HTTP handler).

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use clap::Args;
use ligate_client::submit::Submitter;
use ligate_rollup::MockRollupSpec;
use ligate_stf::runtime::RuntimeCall;
use serde::Serialize;
use sov_bank::{Amount, CallMessage as BankCall, Coins};
use sov_modules_api::capabilities::UniquenessData;
use sov_modules_api::execution_mode::Native;
use sov_modules_api::transaction::{PriorityFeeBips, UnsignedTransaction};
use sov_modules_api::{CryptoSpec, PrivateKey, Spec};

use crate::cli::GlobalArgs;
use crate::config::{parse_chain_hash, parse_token_id};
use crate::keystore::{read_address, resolve_signer_key};

/// Concrete spec mirrors the faucet's choice. `MockRollupSpec<Native>`
/// shares the chain's address shape and runtime composition; the DA
/// flavour (Mock vs Celestia) is a node-side property and does not
/// affect the chain-hash that binds tx signatures.
type S = MockRollupSpec<Native>;
type ChainRuntime = ligate_stf::runtime::Runtime<S>;
type SovPrivateKey = <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey;
type SovAddress = <S as Spec>::Address;

/// Default per-tx fee envelope (in nano-LGT). Generous so a transfer
/// never fails for fee reasons under devnet conditions. Overridable
/// via `--max-fee` on the CLI.
const DEFAULT_MAX_FEE_NANO: u128 = 100_000_000; // 0.1 $LGT

#[derive(Debug, Args)]
pub struct TransferCmd {
    /// Recipient `lig1...` address.
    #[arg(long)]
    pub to: String,

    /// Amount in `$LGT` (decimal). Use `--amount-nano` for the raw
    /// integer form.
    #[arg(long, conflicts_with = "amount_nano")]
    pub amount: Option<f64>,

    /// Amount in nano-LGT (integer). Use `--amount` for the decimal
    /// form. One of the two is required.
    #[arg(long, conflicts_with = "amount")]
    pub amount_nano: Option<u128>,

    /// Local keystore role (created by `ligate keys generate`).
    #[arg(long)]
    pub signer: String,

    /// Override the keystore directory.
    #[arg(long)]
    pub keystore: Option<PathBuf>,

    /// Numeric chain id (u64). Pull from the chain's
    /// `chain_state.json` at genesis. NOT the `chain_id` STRING.
    #[arg(long, env = "LIGATE_CHAIN_ID")]
    pub chain_id: u64,

    /// 32-byte chain hash (64-char hex). Pull from a running node's
    /// `/v1/rollup/info`.
    #[arg(long, env = "LIGATE_CHAIN_HASH")]
    pub chain_hash: String,

    /// `$LGT` token id (64-char hex). Pull from `bank.json`'s
    /// `token_id` at genesis.
    #[arg(long, env = "LIGATE_LGT_TOKEN_ID")]
    pub token_id: String,

    /// Override the per-tx max fee (nano-LGT). Default 100_000_000
    /// (= 0.1 $LGT).
    #[arg(long)]
    pub max_fee: Option<u128>,
}

#[derive(Serialize)]
struct TransferJson<'a> {
    from: String,
    to: &'a str,
    tx_hash: String,
    amount_nano: u128,
    amount_lgt: f64,
}

impl TransferCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        // Resolve amount from one of the two flag forms.
        let amount_nano: u128 = match (self.amount, self.amount_nano) {
            (Some(_), Some(_)) => {
                // clap conflicts_with should have caught this, but be defensive.
                anyhow::bail!("pass --amount or --amount-nano, not both");
            }
            (None, None) => anyhow::bail!("must pass --amount or --amount-nano"),
            (Some(lgt), None) => {
                if lgt < 0.0 {
                    anyhow::bail!("--amount must be non-negative, got {lgt}");
                }
                (lgt * 1_000_000_000.0) as u128
            }
            (None, Some(nano)) => nano,
        };

        // Resolve chain identity.
        let chain_hash = parse_chain_hash(&self.chain_hash)?;
        let token_id = parse_token_id(&self.token_id)?;

        // Resolve signer key + sender address from keystore.
        let key_hex = resolve_signer_key(&self.signer, self.keystore.as_deref())
            .with_context(|| format!("loading key for role '{}'", self.signer))?;
        let from_addr = read_address(
            &self.keystore.clone().unwrap_or_else(|| {
                directories::ProjectDirs::from("io", "ligate", "cli")
                    .map(|d| d.data_dir().join("keys"))
                    .unwrap_or_default()
            }),
            &self.signer,
        )?;

        let key_bytes = hex::decode(&key_hex).context("hex-decoding signer key")?;
        let private_key =
            SovPrivateKey::try_from(key_bytes).map_err(|e| anyhow::anyhow!("key shape: {e:?}"))?;

        // Build runtime call.
        let to: SovAddress = SovAddress::from_str(&self.to)
            .with_context(|| format!("parsing --to address {}", self.to))?;
        let runtime_call: RuntimeCall<S> = RuntimeCall::Bank(BankCall::Transfer {
            to,
            coins: Coins {
                amount: Amount::from(amount_nano),
                token_id,
            },
        });

        // Connect + fetch nonce. Unlike the faucet (which keeps an
        // in-memory atomic counter), the CLI is one-shot per
        // invocation, so we re-fetch every time.
        let submitter = Submitter::new(&global.rpc)
            .await
            .with_context(|| format!("connecting to {}", global.rpc))?;
        let nonce = submitter
            .inner()
            .get_nonce_for_public_key::<S>(&private_key.pub_key())
            .await
            .with_context(|| format!("fetching nonce for {from_addr}"))?;

        // Wrap, sign, encode, submit.
        let max_fee = self.max_fee.unwrap_or(DEFAULT_MAX_FEE_NANO);
        let unsigned = UnsignedTransaction::<ChainRuntime, S>::new(
            runtime_call,
            self.chain_id,
            PriorityFeeBips::ZERO,
            Amount::from(max_fee),
            UniquenessData::Nonce(nonce),
            None,
        );
        let signed = unsigned.sign(&private_key, &chain_hash);
        // Borsh-encode the signed `Transaction`. The chain's
        // `POST /v1/sequencer/txs` handler accepts the inner signed tx
        // bytes directly and wraps them in `AuthenticatorInput::Standard`
        // server-side (see `sov-sequencer::rest_api::axum_accept_tx`).
        // Pre-wrapping here would double-wrap and the chain would
        // reject with "Cannot decompress Edwards point" (chain #245).
        let signed_bytes = borsh::to_vec(&signed).context("encoding signed tx")?;
        let tx_hash = submitter
            .submit_raw_tx(signed_bytes, true)
            .await
            .with_context(|| format!("submitting transfer {} -> {}", from_addr, self.to))?;

        // Progenitor's pattern-validated string types impl `ToString`
        // (so `.to_string()` works) but not `Display` (so `{tx_hash}`
        // wouldn't). Convert once to a String and format that.
        let tx_hash_str = tx_hash.to_string();
        let amount_lgt = (amount_nano as f64) / 1_000_000_000.0;

        if global.json {
            let payload = TransferJson {
                from: from_addr.clone(),
                to: &self.to,
                tx_hash: tx_hash_str.clone(),
                amount_nano,
                amount_lgt,
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("Transfer accepted by chain:");
            println!("  from:   {from_addr}");
            println!("  to:     {}", self.to);
            println!("  amount: {amount_lgt} $LGT ({amount_nano} nano)");
            println!("  tx:     {tx_hash_str}");
        }
        Ok(())
    }
}
