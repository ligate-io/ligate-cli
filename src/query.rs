//! `ligate query` subcommand: read-side lookups against the chain's
//! attestation REST API.
//!
//! Pure read path: no signing, no keystore. Three point lookups
//! mounted by the chain under `/v1/modules/attestation/...`:
//!
//! - `query schema <lsc1...>` -> `GET /modules/attestation/schemas/{id}`
//! - `query attestor-set <las1...>` -> `GET /modules/attestation/attestor-sets/{id}`
//! - `query attestation <lat1...>` -> `GET /modules/attestation/attestations/{id}`
//!
//! Each id is validated client-side before the request so an obvious
//! typo fails fast without a network round-trip. The chain's typed
//! JSON response is pretty-printed verbatim; a 404 is reported as
//! "not found" rather than a raw HTTP error.

use std::str::FromStr;

use anyhow::{Context, Result};
use clap::Subcommand;
use ligate_client::{AttestationId, AttestorSetId, SchemaId};

use crate::cli::GlobalArgs;

#[derive(Debug, Subcommand)]
pub enum QueryCmd {
    /// Fetch a registered schema by its bech32m `lsc1...` id.
    Schema {
        /// Bech32m `lsc1...` schema id.
        id: String,
    },

    /// Fetch a registered attestor set by its bech32m `las1...` id.
    AttestorSet {
        /// Bech32m `las1...` attestor set id.
        id: String,
    },

    /// Fetch an attestation by its bech32m `lat1...` id.
    Attestation {
        /// Bech32m `lat1...` attestation id (32-byte hash of schema_id || payload_hash).
        id: String,
    },
}

impl QueryCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        let rpc = global.rpc_with_v1();

        // Validate the id client-side, then build the REST URL from the
        // canonical (re-encoded) form so a stray-whitespace or wrong-HRP
        // id fails here rather than as a chain-side 400.
        let url = match &self {
            QueryCmd::Schema { id } => {
                let parsed =
                    SchemaId::from_str(id).with_context(|| format!("parsing schema id '{id}'"))?;
                format!("{rpc}/modules/attestation/schemas/{parsed}")
            }
            QueryCmd::AttestorSet { id } => {
                let parsed = AttestorSetId::from_str(id)
                    .with_context(|| format!("parsing attestor set id '{id}'"))?;
                format!("{rpc}/modules/attestation/attestor-sets/{parsed}")
            }
            QueryCmd::Attestation { id } => {
                let parsed = AttestationId::from_str(id)
                    .with_context(|| format!("parsing attestation id '{id}'"))?;
                format!("{rpc}/modules/attestation/attestations/{parsed}")
            }
        };

        query_get(&url).await
    }
}

/// Fire a `GET`, pretty-print a 2xx JSON body, and surface 404 as a
/// clean "not found" instead of a raw HTTP error.
///
/// Uses `reqwest` directly rather than the SDK's `http_get` because
/// the latter returns `Err` on 404, which is ambiguous with a network
/// failure (same reason the chain's `bootstrap-cli` does this). The
/// `Client::new` + `.get().send()` shape mirrors `faucet.rs`.
async fn query_get(url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("HTTP GET {url}"))?;
    let status = resp.status();

    if status == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("not found (404): nothing registered at {url}");
    }

    // Read the body once, then branch (mirrors `faucet.rs`).
    let body = resp.text().await.context("reading response body")?;
    if !status.is_success() {
        anyhow::bail!("unexpected status {status} from {url}: {body}");
    }

    // Pretty-print when the body parses as JSON; fall back to raw text
    // so an unexpected non-JSON response is still visible.
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        Err(_) => println!("{body}"),
    }
    Ok(())
}
