//! `ligate register-schema` subcommand: register an attestation
//! schema from a JSON definition file.
//!
//! A schema is the "what shape, what rules" half of an attestation: a
//! name + version, the attestor set whose quorum signs attestations
//! under it, optional builder-fee routing, and an opaque 32-byte
//! content address pinning the off-chain payload shape.
//!
//! ## `--file` JSON shape
//!
//! ```json
//! {
//!   "name": "themisra.proof-of-prompt",
//!   "version": 1,
//!   "attestor_set_id": "las1...",
//!   "fee_routing_bps": 0,
//!   "fee_routing_addr": null,
//!   "payload_shape_hash": null
//! }
//! ```
//!
//! `name`, `version`, and `attestor_set_id` are required. The rest
//! default: `fee_routing_bps` to 0, `fee_routing_addr` to none, and
//! `payload_shape_hash` to all-zeros (the documented opt-out for the
//! off-chain payload-spec content address). `payload_shape_hash`, when
//! present, is 32 bytes of hex (with or without a `0x` prefix).
//!
//! The owner of a registered schema is the transaction submitter, so
//! the `lsc1...` schema id derives from the signer's address + name +
//! version. We compute it locally to echo it back.

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use clap::Args;
use ligate_client::{AttestorSetId, Schema};
use ligate_stf::runtime::RuntimeCall;
use serde::{Deserialize, Serialize};

use crate::attest::{
    build_sign_submit, resolve_signer, signer_address, ChainArgs, SignerArgs, SovAddress, S,
};
use crate::cli::GlobalArgs;

#[derive(Debug, Args)]
pub struct RegisterSchemaCmd {
    /// Path to the schema definition JSON (shape documented in the
    /// module docstring).
    #[arg(long)]
    pub file: PathBuf,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[command(flatten)]
    pub chain: ChainArgs,
}

/// On-disk shape of the `--file` schema definition.
#[derive(Debug, Deserialize)]
struct SchemaDoc {
    /// Schema name, e.g. `themisra.proof-of-prompt`.
    name: String,
    /// Schema version. Bumping the version is how a schema "rotates"
    /// to a new attestor set.
    version: u32,
    /// Bech32m `las1...` id of an already-registered attestor set.
    attestor_set_id: String,
    /// Builder-fee routing in basis points. Defaults to 0 (no routing).
    #[serde(default)]
    fee_routing_bps: u16,
    /// `lig1...` address builder fees route to. Required iff
    /// `fee_routing_bps > 0`.
    #[serde(default)]
    fee_routing_addr: Option<String>,
    /// 32-byte hex content address of the off-chain payload spec.
    /// Omit (or null) to opt out; the chain stores it verbatim and
    /// never verifies it.
    #[serde(default)]
    payload_shape_hash: Option<String>,
}

#[derive(Serialize)]
struct RegisterSchemaJson {
    schema_id: String,
    name: String,
    version: u32,
    attestor_set_id: String,
    tx_hash: String,
}

/// Parse a 32-byte hex content hash (with or without a `0x` prefix).
fn parse_payload_shape_hash(s: &str) -> Result<[u8; 32]> {
    let cleaned = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let bytes = hex::decode(cleaned).context("hex-decoding payload_shape_hash")?;
    bytes.try_into().map_err(|v: Vec<u8>| {
        anyhow::anyhow!("payload_shape_hash must be 32 bytes, got {}", v.len())
    })
}

impl RegisterSchemaCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        let raw = fs::read_to_string(&self.file)
            .with_context(|| format!("reading schema file {}", self.file.display()))?;
        let doc: SchemaDoc = serde_json::from_str(&raw)
            .with_context(|| format!("parsing schema JSON from {}", self.file.display()))?;

        // Fee-routing invariant: an address is required iff a non-zero
        // share is routed. The chain enforces this too; checking here
        // saves a failed tx + its fee.
        if (doc.fee_routing_bps > 0) != doc.fee_routing_addr.is_some() {
            anyhow::bail!(
                "fee_routing_addr must be set iff fee_routing_bps > 0 \
                 (bps = {}, addr = {:?})",
                doc.fee_routing_bps,
                doc.fee_routing_addr
            );
        }

        let attestor_set_id = AttestorSetId::from_str(&doc.attestor_set_id)
            .with_context(|| format!("parsing attestor_set_id '{}'", doc.attestor_set_id))?;
        let fee_routing_addr: Option<SovAddress> = doc
            .fee_routing_addr
            .as_deref()
            .map(|s| {
                SovAddress::from_str(s)
                    .map_err(|e| anyhow::anyhow!("parsing fee_routing_addr '{s}': {e:?}"))
            })
            .transpose()?;
        let payload_shape_hash = match doc.payload_shape_hash.as_deref() {
            Some(s) => parse_payload_shape_hash(s)?,
            None => [0u8; 32],
        };

        let private_key = resolve_signer(&self.signer)?;

        // The schema owner is the tx submitter; the `lsc1...` id derives
        // from owner + name + version. Computed locally for display;
        // the chain re-derives the same id on submission.
        let owner = signer_address(&private_key);
        let schema_id = Schema::<S>::derive_id(&owner, &doc.name, doc.version);

        let call_message = ligate_client::register_schema::<S>(
            doc.name.clone(),
            doc.version,
            attestor_set_id,
            doc.fee_routing_bps,
            fee_routing_addr,
            payload_shape_hash,
        )
        .context("building RegisterSchema call")?;
        let call = RuntimeCall::Attestation(call_message);

        let tx_hash = build_sign_submit(global, &self.chain, &private_key, call).await?;

        if global.json {
            let payload = RegisterSchemaJson {
                schema_id: schema_id.to_string(),
                name: doc.name,
                version: doc.version,
                attestor_set_id: attestor_set_id.to_string(),
                tx_hash,
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("Schema registered:");
            println!("  schema_id:       {schema_id}");
            println!("  name:            {} v{}", doc.name, doc.version);
            println!("  attestor_set_id: {attestor_set_id}");
            println!("  tx:              {tx_hash}");
        }
        Ok(())
    }
}
