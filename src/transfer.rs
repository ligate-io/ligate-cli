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
use std::time::{Duration, Instant};

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
use crate::nonce::fetch_account_nonce;
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
    /// One of `--signer` or `--private-key-hex` is required.
    #[arg(long)]
    pub signer: Option<String>,

    /// Override the keystore directory.
    #[arg(long)]
    pub keystore: Option<PathBuf>,

    /// Numeric chain id (u64). Pull from the chain's
    /// `chain_state.json` at genesis. NOT the `chain_id` STRING.
    #[arg(long, env = "LIGATE_CHAIN_ID")]
    pub chain_id: u64,

    /// 32-byte chain hash. Accepts bech32m `lsch1...` (canonical since
    /// `ligate-chain@0ac7e5b`), `0x`-prefixed hex, or bare 64-char
    /// hex. Pull from a running node's `/v1/rollup/info`, or directly
    /// from `ligate info --json | jq -r .chain_hash`.
    #[arg(long, env = "LIGATE_CHAIN_HASH")]
    pub chain_hash: String,

    /// `$LGT` token id. Accepts bech32m `token_1...` (canonical since
    /// `ligate-chain@0ac7e5b`), `0x`-prefixed hex, or bare 64-char
    /// hex. Pull from `bank.json`'s `token_id` at genesis, the
    /// explorer, or `ligate balance --json`.
    #[arg(long, env = "LIGATE_LGT_TOKEN_ID")]
    pub token_id: String,

    /// Override the per-tx max fee (nano-LGT). Default 100_000_000
    /// (= 0.1 $LGT).
    #[arg(long)]
    pub max_fee: Option<u128>,

    /// Build + sign the transfer locally, print the hex-encoded signed
    /// tx bytes to stdout, and exit. Skips the RPC roundtrip entirely
    /// (no nonce fetch, no submit, no inclusion wait).
    ///
    /// Used by the byte-level parity test against `@ligate-labs/sdk`
    /// (ligate-js#18): same inputs through both signers should produce
    /// byte-identical signed-tx bytes. Mismatches indicate wire-format
    /// drift between the Rust + TS impls.
    ///
    /// Requires `--nonce` (can't fetch from chain in offline mode) and
    /// either `--signer` reading from the keystore OR `--private-key-hex`
    /// passing the seed inline.
    #[arg(long, requires = "nonce")]
    pub print_tx_bytes: bool,

    /// Account nonce. Required with `--print-tx-bytes`; otherwise the
    /// CLI fetches it from chain.
    #[arg(long)]
    pub nonce: Option<u64>,

    /// 32-byte private key seed as hex (with or without `0x` prefix).
    /// Alternative to `--signer` for offline / parity-test flows that
    /// don't want to set up a keystore. Conflicts with `--signer`.
    #[arg(long, conflicts_with = "signer")]
    pub private_key_hex: Option<String>,
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

        // Resolve signer key. Two paths:
        //
        // 1. `--signer <role>` reads from the keystore (production path)
        // 2. `--private-key-hex 0x..` takes the seed inline (parity-test
        //    + offline scripting path)
        //
        // Exactly one is required; clap enforces conflict.
        let (key_hex, from_addr_opt) = match (&self.signer, &self.private_key_hex) {
            (Some(_), Some(_)) => {
                anyhow::bail!("pass --signer or --private-key-hex, not both");
            }
            (Some(signer), None) => {
                let key = resolve_signer_key(signer, self.keystore.as_deref())
                    .with_context(|| format!("loading key for role '{}'", signer))?;
                let addr = read_address(
                    &self.keystore.clone().unwrap_or_else(|| {
                        directories::ProjectDirs::from("io", "ligate", "cli")
                            .map(|d| d.data_dir().join("keys"))
                            .unwrap_or_default()
                    }),
                    signer,
                )?;
                (key, Some(addr))
            }
            (None, Some(hex_seed)) => {
                let cleaned = hex_seed
                    .strip_prefix("0x")
                    .or_else(|| hex_seed.strip_prefix("0X"))
                    .unwrap_or(hex_seed);
                // `--private-key-hex` is the offline / parity-test
                // path; sender address derives from the key but we
                // don't need it for the signed-bytes output, so leave
                // the keystore-read out of band.
                (cleaned.to_string(), None)
            }
            (None, None) => {
                anyhow::bail!("must pass --signer or --private-key-hex");
            }
        };

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

        // Resolve nonce. Two paths matching the signer flag:
        //
        // - `--nonce N` (required by `--print-tx-bytes`): use the
        //   supplied value. Offline mode, no RPC.
        // - default: fetch from chain via [`fetch_account_nonce`]
        //   (queries the chain's uniqueness module directly; the SDK
        //   fork's `get_nonce_for_public_key` targets the wrong path
        //   and silently returns 0 on the resulting 404).
        let max_fee = self.max_fee.unwrap_or(DEFAULT_MAX_FEE_NANO);

        if self.print_tx_bytes {
            // Offline build + sign + print path. `--nonce` is required
            // by clap; unwrap is safe (clap enforces `requires =
            // "nonce"` on the --print-tx-bytes flag).
            let nonce = self
                .nonce
                .ok_or_else(|| anyhow::anyhow!("--print-tx-bytes requires --nonce"))?;
            let unsigned = UnsignedTransaction::<ChainRuntime, S>::new(
                runtime_call,
                self.chain_id,
                PriorityFeeBips::ZERO,
                Amount::from(max_fee),
                UniquenessData::Nonce(nonce),
                None,
            );
            let signed = unsigned.sign(&private_key, &chain_hash);
            let signed_bytes = borsh::to_vec(&signed).context("encoding signed tx")?;
            // Hex (no prefix) is the canonical interchange form for the
            // ligate-js parity test. lowercase, no newline before the
            // final one (println adds it).
            println!("{}", hex::encode(&signed_bytes));
            return Ok(());
        }

        // Online path: connect + fetch nonce. Unlike the faucet
        // (which keeps an in-memory atomic counter), the CLI is
        // one-shot per invocation, so we re-fetch every time.
        let from_addr =
            from_addr_opt.expect("online path requires --signer (keystore-derived address)");
        let rpc = global.rpc_with_v1();
        let submitter = Submitter::new(&rpc)
            .await
            .with_context(|| format!("connecting to {rpc}"))?;
        let nonce = match self.nonce {
            Some(n) => n,
            None => fetch_account_nonce::<S>(&submitter, &private_key.pub_key())
                .await
                .with_context(|| format!("fetching nonce for {from_addr}"))?,
        };

        // Wrap, sign, encode, submit.
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
        // Pass `wait_for_inclusion = false`. The SDK's true path uses
        // a WebSocket subscription (`subscribe_to_tx_status_updates`)
        // that hits a URL-parsing bug in our setup (`invalid port
        // value`, see issue #8). We do an HTTP poll on
        // `/v1/ledger/txs/{hash}` instead — same UX, no WebSocket.
        let tx_hash = submitter
            .submit_raw_tx(signed_bytes, false)
            .await
            .with_context(|| format!("submitting transfer {} -> {}", from_addr, self.to))?;

        // Progenitor's pattern-validated string types impl `ToString`
        // (so `.to_string()` works) but not `Display` (so `{tx_hash}`
        // wouldn't). Convert once to a String and format that.
        let tx_hash_str = tx_hash.to_string();

        // Poll for inclusion. Returns once the chain has indexed the
        // tx (success or failure both count) or times out.
        wait_for_inclusion_via_http(&submitter, &rpc, &tx_hash_str).await?;
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

/// Poll the chain via HTTP `GET /ledger/txs/{tx_hash}` until the
/// transaction has been indexed. Returns once we see a 2xx response.
///
/// Replaces the SDK's `wait_for_tx_processing`, which subscribes via
/// WebSocket and trips a URL-parse bug (`invalid port value`) when
/// constructing the `ws://` upgrade URL from a non-standard
/// `http://host:port` base. The chain itself accepts the tx fine in
/// either case; this is purely a client-side confirmation lookup, so
/// HTTP polling is functionally equivalent and avoids the SDK's WS
/// path entirely.
///
/// `rpc_with_v1` is the URL produced by [`GlobalArgs::rpc_with_v1`]
/// (already ends in `/v1`).
///
/// Times out after 30s. Polls every 500ms with bounded backoff.
async fn wait_for_inclusion_via_http(
    submitter: &Submitter,
    rpc_with_v1: &str,
    tx_hash: &str,
) -> Result<()> {
    const POLL_INTERVAL: Duration = Duration::from_millis(500);
    const MAX_WAIT: Duration = Duration::from_secs(30);

    // `NodeClient::http_get` prepends its own `base_url`, so we pass
    // the PATH `/ledger/txs/<hash>`, not the full URL. See `info.rs`
    // and the sister fix in `attest.rs` for the same root cause:
    // passing the full URL produced a doubled URL the chain returned
    // 404 for with an empty body, and `http_get` returned `Ok("")`
    // because it doesn't check status -- exiting the polling loop on
    // the first poll and falsely reporting inclusion.
    let full_url = format!("{rpc_with_v1}/ledger/txs/{tx_hash}"); // error msg only
    let path = format!("/ledger/txs/{tx_hash}");
    let started = Instant::now();
    loop {
        if started.elapsed() > MAX_WAIT {
            anyhow::bail!(
                "timed out after {:?} waiting for tx {tx_hash} to be included; \
                 the tx may still land — check `{full_url}` to verify",
                MAX_WAIT
            );
        }
        // `http_get` returns `Ok("")` on 404 (SDK doesn't check
        // status). Empty body == tx not yet indexed == keep polling.
        // A populated body == chain returning the indexed tx JSON.
        match submitter.inner().http_get(&path).await {
            Ok(body) if !body.trim().is_empty() => return Ok(()),
            _ => {}
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
