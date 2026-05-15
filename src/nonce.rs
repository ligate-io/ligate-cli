//! Shared on-chain nonce-fetch helper, used by the write subcommands
//! that need to seed an `UnsignedTransaction` with the current
//! account nonce before signing.
//!
//! Exists as a workaround for the in-house SDK fork's broken
//! [`NodeClient::get_nonce_for_public_key`]. The SDK targets
//! `/modules/nonces/...` (the upstream module name) but the fork
//! renamed the module to `sov-uniqueness`, exposed at
//! `/modules/uniqueness/...`. The 404 the SDK gets back is silently
//! mapped to `nonce=0` by `error_for_status` (and a `match
//! response.error_for_status() { Err(_) => 0 }` arm), so any account
//! that's sent more than one tx then hits
//! `Tx bad nonce: expected: N, but found: 0`.
//!
//! Tracked in `ligate-io/sovereign-sdk#TBD`. Once that lands and
//! `ligate-chain` bumps its fork pin, this module collapses into a
//! single call to `submitter.inner().get_nonce_for_public_key::<S>(..)`
//! and the workaround can be deleted.
//!
//! [`NodeClient::get_nonce_for_public_key`]: https://github.com/Sovereign-Labs/sovereign-sdk/blob/main/crates/utils/sov-node-client/src/lib.rs

use anyhow::{Context, Result};
use ligate_client::submit::Submitter;
use serde::Deserialize;
use sov_modules_api::{CryptoSpec, PublicKey, Spec};

/// Fetch the next-to-use nonce for `pub_key` directly off the chain's
/// `/modules/uniqueness/state/nonces/items/{credential_id}` endpoint.
///
/// `S` is the concrete spec the caller uses to construct
/// `UnsignedTransaction` (typically `MockRollupSpec<Native>` per
/// `transfer.rs` and `attest.rs`).
pub async fn fetch_account_nonce<S: Spec>(
    submitter: &Submitter,
    pub_key: &<<S as Spec>::CryptoSpec as CryptoSpec>::PublicKey,
) -> Result<u64> {
    let cred_id = pub_key.credential_id();
    let path = format!("/modules/uniqueness/state/nonces/items/{cred_id}");
    let body = submitter
        .inner()
        .http_get(&path)
        .await
        .with_context(|| format!("fetching account nonce via {path}"))?;
    // `http_get` returns `Ok("")` on 404 (chain returns 404 for accounts
    // that have never sent a tx). Treat empty body as nonce=0.
    if body.trim().is_empty() {
        return Ok(0);
    }
    #[derive(Deserialize)]
    struct NonceResp {
        value: u64,
    }
    let resp: NonceResp = serde_json::from_str(&body)
        .with_context(|| format!("parsing nonce response body: {body}"))?;
    Ok(resp.value)
}
