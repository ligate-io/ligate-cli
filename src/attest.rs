//! Shared plumbing for the attestation write subcommands
//! (`register-attestor-set`, `register-schema`, `submit-attestation`).
//!
//! The three attestation write verbs are the same shape as `transfer`:
//! resolve a signing key, build a `RuntimeCall`, wrap it in an
//! `UnsignedTransaction`, sign against the chain hash, borsh-encode,
//! submit to the sequencer, poll for inclusion. The only thing that
//! differs per verb is which `RuntimeCall::Attestation(..)` variant
//! gets built. This module holds everything common so each verb module
//! stays a thin "parse args -> build call -> [`build_sign_submit`]".
//!
//! Mirrors `transfer.rs` and the chain's `bootstrap-cli/register.rs`
//! build-sign-submit pipeline byte-for-byte so the borsh shape we sign
//! matches what the chain re-derives at verify time.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Args;
use ligate_client::submit::Submitter;
use ligate_rollup::MockRollupSpec;
use ligate_stf::runtime::RuntimeCall;
use sov_modules_api::capabilities::UniquenessData;
use sov_modules_api::execution_mode::Native;
use sov_modules_api::transaction::{PriorityFeeBips, UnsignedTransaction};
use sov_modules_api::{Amount, CryptoSpec, PrivateKey, PublicKey, Spec};

use crate::cli::GlobalArgs;
use crate::config::parse_chain_hash;
use crate::keystore::resolve_signer_key;
use crate::nonce::fetch_account_nonce;

/// Concrete spec, identical to `transfer.rs` and the chain's
/// `bootstrap-cli`. `MockRollupSpec<Native>` shares the chain's
/// address shape and runtime composition; the DA flavour (Mock vs
/// Celestia) is a node-side property and does not affect the
/// chain-hash that binds tx signatures.
pub type S = MockRollupSpec<Native>;
/// Wrapped runtime the `UnsignedTransaction` generic is pinned to.
pub type ChainRuntime = ligate_stf::runtime::Runtime<S>;
/// Spec-derived private key type accepted by `UnsignedTransaction::sign`.
pub type SovPrivateKey = <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey;
/// Spec-derived account address type.
pub type SovAddress = <S as Spec>::Address;

/// Default per-tx fee envelope (nano-LGT). Generous so a registration
/// never fails for fee reasons under devnet conditions. Slightly
/// higher than `transfer`'s default because schema/attestor-set
/// registration fees are larger than a bare transfer. Overridable via
/// `--max-fee`.
pub const DEFAULT_MAX_FEE_NANO: u128 = 200_000_000; // 0.2 $LGT

/// Signing-key resolution flags, flattened into each write subcommand.
///
/// Exactly one of `--signer` / `--private-key-hex` is required; clap
/// enforces the conflict. Mirrors `transfer`'s flags so an operator's
/// muscle memory carries over.
#[derive(Debug, Args)]
pub struct SignerArgs {
    /// Local keystore role (created by `ligate keys generate`).
    /// One of `--signer` or `--private-key-hex` is required.
    #[arg(long)]
    pub signer: Option<String>,

    /// Override the keystore directory.
    #[arg(long)]
    pub keystore: Option<PathBuf>,

    /// 32-byte private key seed as hex (with or without `0x` prefix).
    /// Alternative to `--signer` for offline / scripted flows that
    /// don't want to set up a keystore. Conflicts with `--signer`.
    #[arg(long, conflicts_with = "signer")]
    pub private_key_hex: Option<String>,
}

/// Chain-identity flags, flattened into each write subcommand.
///
/// Equivalent to `transfer`'s `--chain-id` / `--chain-hash` /
/// `--max-fee`. Pulled from a running node via `ligate info --json`.
#[derive(Debug, Args)]
pub struct ChainArgs {
    /// Numeric chain id (u64). Pull from the chain's
    /// `chain_state.json` at genesis. NOT the `chain_id` STRING.
    #[arg(long, env = "LIGATE_CHAIN_ID")]
    pub chain_id: u64,

    /// 32-byte chain hash. Accepts bech32m `lsch1...`, `0x`-prefixed
    /// hex, or bare 64-char hex. Pull from a running node's
    /// `/v1/rollup/info`, or `ligate info --json | jq -r .chain_hash`.
    #[arg(long, env = "LIGATE_CHAIN_HASH")]
    pub chain_hash: String,

    /// Override the per-tx max fee (nano-LGT). Default 200_000_000
    /// (= 0.2 $LGT).
    #[arg(long)]
    pub max_fee: Option<u128>,

    /// Override the account nonce instead of fetching from chain.
    ///
    /// Escape hatch for the SDK fork's broken `get_nonce_for_public_key`
    /// (queries `/modules/nonces/...` against a chain that exposes the
    /// renamed `/modules/uniqueness/...` path; the 404 silently maps to
    /// 0). The local `fetch_account_nonce` helper below routes around
    /// that, so users normally do not need this flag; keep it for
    /// emergency overrides and offline build-sign-print flows that
    /// might land here later.
    #[arg(long)]
    pub nonce: Option<u64>,
}

/// Resolve a [`SovPrivateKey`] from the `--signer` / `--private-key-hex`
/// flags.
///
/// Two paths, mirroring `transfer.rs`:
///
/// 1. `--signer <role>` reads the hex seed from the keystore.
/// 2. `--private-key-hex 0x..` takes the seed inline.
///
/// Exactly one is required; clap enforces the conflict, this function
/// is defensive about the remaining cases.
pub fn resolve_signer(args: &SignerArgs) -> Result<SovPrivateKey> {
    let key_hex = match (&args.signer, &args.private_key_hex) {
        (Some(_), Some(_)) => {
            anyhow::bail!("pass --signer or --private-key-hex, not both");
        }
        (Some(role), None) => resolve_signer_key(role, args.keystore.as_deref())
            .with_context(|| format!("loading key for role '{role}'"))?,
        (None, Some(hex_seed)) => {
            let cleaned = hex_seed
                .strip_prefix("0x")
                .or_else(|| hex_seed.strip_prefix("0X"))
                .unwrap_or(hex_seed);
            cleaned.to_string()
        }
        (None, None) => {
            anyhow::bail!("must pass --signer or --private-key-hex");
        }
    };
    let key_bytes = hex::decode(&key_hex).context("hex-decoding signer key")?;
    SovPrivateKey::try_from(key_bytes).map_err(|e| anyhow::anyhow!("key shape: {e:?}"))
}

/// Derive the chain account address from a signer's public key.
///
/// Mirrors the chain's authentication path:
/// `Address::from(pubkey.credential_id())`, lifted verbatim from
/// `bootstrap-cli/register.rs`. Used to compute the deterministic
/// `lsc1...` schema id for display (the owner of a registered schema
/// is the transaction submitter).
pub fn signer_address(private_key: &SovPrivateKey) -> SovAddress {
    let pubkey = private_key.pub_key();
    SovAddress::from(pubkey.credential_id())
}

/// Wrap -> sign -> encode -> submit -> poll a single [`RuntimeCall`].
///
/// The one network-touching helper the write verbs share. Connects to
/// the RPC, fetches the signer's nonce, builds + signs the
/// transaction, submits it to the sequencer, and polls
/// `GET /ledger/txs/{hash}` until the chain has indexed it. Returns the
/// transaction hash string.
pub async fn build_sign_submit(
    global: &GlobalArgs,
    chain: &ChainArgs,
    private_key: &SovPrivateKey,
    call: RuntimeCall<S>,
) -> Result<String> {
    let chain_hash = parse_chain_hash(&chain.chain_hash)?;
    let max_fee = chain.max_fee.unwrap_or(DEFAULT_MAX_FEE_NANO);

    let rpc = global.rpc_with_v1();
    let submitter = Submitter::new(&rpc)
        .await
        .with_context(|| format!("connecting to {rpc}"))?;

    // One-shot per invocation, so re-fetch the nonce every time (the
    // chain is the source of truth; no in-memory counter to drift).
    //
    // `--nonce N` overrides the chain fetch. Used as an escape hatch
    // for the SDK fork bug below + for future offline build-sign-print
    // flows.
    let nonce = match chain.nonce {
        Some(n) => n,
        None => fetch_account_nonce::<S>(&submitter, &private_key.pub_key()).await?,
    };

    let unsigned = UnsignedTransaction::<ChainRuntime, S>::new(
        call,
        chain.chain_id,
        PriorityFeeBips::ZERO,
        Amount::from(max_fee),
        UniquenessData::Nonce(nonce),
        None,
    );
    let signed = unsigned.sign(private_key, &chain_hash);
    // Borsh-encode the signed `Transaction`. The chain's
    // `POST /v1/sequencer/txs` handler wraps the inner signed-tx bytes
    // in `AuthenticatorInput::Standard` server-side; pre-wrapping here
    // would double-wrap and the chain would reject (chain #245).
    let signed_bytes = borsh::to_vec(&signed).context("encoding signed tx")?;
    let tx_hash = submitter
        .submit_raw_tx(signed_bytes, false)
        .await
        .context("submitting tx to sequencer")?;
    // Progenitor's pattern-validated string types impl `ToString` but
    // not `Display`; convert once and format the String.
    let tx_hash_str = tx_hash.to_string();

    wait_for_inclusion_via_http(&submitter, &rpc, &tx_hash_str).await?;
    Ok(tx_hash_str)
}

/// Poll `GET {rpc}/ledger/txs/{tx_hash}` until the transaction has been
/// indexed by the chain. Returns once a 2xx response comes back.
///
/// Same HTTP-poll pattern as `transfer.rs` and the chain's
/// `bootstrap-cli`. Avoids the SDK's WebSocket `wait_for_tx_processing`
/// path, which trips a URL-parse bug (`invalid port value`) against
/// non-standard `http://host:port` bases (ligate-cli#8).
///
/// Times out after 30s. Polls every 500ms.
async fn wait_for_inclusion_via_http(
    submitter: &Submitter,
    rpc_with_v1: &str,
    tx_hash: &str,
) -> Result<()> {
    const POLL_INTERVAL: Duration = Duration::from_millis(500);
    const MAX_WAIT: Duration = Duration::from_secs(30);

    // `NodeClient::http_get` prepends its own `base_url`, so we pass
    // the PATH `/ledger/txs/<hash>`, not the full URL. The earlier
    // version (`format!("{rpc_with_v1}/ledger/txs/{tx_hash}")`)
    // produced a doubled URL the chain returned 404 for with an empty
    // body -- and `http_get` returns `Ok("")` on 404 because it
    // doesn't check status. The `.is_ok()` check then exited the
    // loop on the first poll, falsely reporting the tx as included.
    // See `info.rs` for the same root cause + fix.
    let full_url = format!("{rpc_with_v1}/ledger/txs/{tx_hash}"); // error msg only
    let path = format!("/ledger/txs/{tx_hash}");
    let started = Instant::now();
    loop {
        if started.elapsed() > MAX_WAIT {
            anyhow::bail!(
                "timed out after {:?} waiting for tx {tx_hash} to be included; \
                 the tx may still land. Check `{full_url}` to verify",
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
