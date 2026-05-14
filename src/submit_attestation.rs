//! `ligate submit-attestation` subcommand: submit a threshold-signed
//! attestation under an existing schema.
//!
//! The CLI does not sign attestations itself: the attestor quorum
//! signs the canonical `SignedAttestationPayload` digest out of band
//! (each attestor with their own key, e.g. via `ligate-js`'s
//! `signSubmitAttestation` or `ligate_client::sign_attestation`), and
//! their signatures are collected into a JSON file passed here. This
//! command bundles them into one `SubmitAttestation` transaction and
//! submits it; the chain validates each signature against the
//! schema's attestor set at execution time.
//!
//! ## `--signatures` JSON shape
//!
//! ```json
//! [
//!   { "pubkey": "lpk1...", "sig": "<hex>" },
//!   { "pubkey": "lpk1...", "sig": "<hex>" }
//! ]
//! ```
//!
//! `pubkey` is the attestor's bech32m `lpk1...` key; `sig` is the raw
//! signature as hex (with or without a `0x` prefix). ed25519 fills 64
//! bytes; up to 96 are accepted for forward compatibility with other
//! schemes.

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use clap::Args;
use ligate_client::{
    AttestationId, AttestorSignature, PayloadHash, PubKey, SchemaId, MAX_ATTESTOR_SIGNATURE_BYTES,
};
use ligate_stf::runtime::RuntimeCall;
use serde::{Deserialize, Serialize};
use sov_modules_api::SafeVec;

use crate::attest::{build_sign_submit, resolve_signer, ChainArgs, SignerArgs, S};
use crate::cli::GlobalArgs;

#[derive(Debug, Args)]
pub struct SubmitAttestationCmd {
    /// Bech32m `lsc1...` id of the schema this attestation is under.
    #[arg(long)]
    pub schema: String,

    /// Bech32m `lph1...` hash of the off-chain payload being attested.
    #[arg(long)]
    pub payload_hash: String,

    /// Path to a JSON file of attestor signatures (shape in the
    /// module docstring).
    #[arg(long)]
    pub signatures: PathBuf,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[command(flatten)]
    pub chain: ChainArgs,
}

/// One entry in the `--signatures` JSON array.
#[derive(Debug, Deserialize)]
struct SignatureEntry {
    /// Attestor's bech32m `lpk1...` public key.
    pubkey: String,
    /// Raw signature as hex (with or without a `0x` prefix).
    sig: String,
}

#[derive(Serialize)]
struct SubmitAttestationJson {
    attestation_id: String,
    schema_id: String,
    payload_hash: String,
    signature_count: usize,
    tx_hash: String,
}

impl SubmitAttestationCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        let schema_id = SchemaId::from_str(&self.schema)
            .with_context(|| format!("parsing --schema '{}'", self.schema))?;
        let payload_hash = PayloadHash::from_str(&self.payload_hash)
            .with_context(|| format!("parsing --payload-hash '{}'", self.payload_hash))?;

        let raw = fs::read_to_string(&self.signatures)
            .with_context(|| format!("reading signatures file {}", self.signatures.display()))?;
        let entries: Vec<SignatureEntry> = serde_json::from_str(&raw).with_context(|| {
            format!("parsing signatures JSON from {}", self.signatures.display())
        })?;
        if entries.is_empty() {
            anyhow::bail!(
                "signatures file {} contains no signatures",
                self.signatures.display()
            );
        }

        let signatures: Vec<AttestorSignature> = entries
            .iter()
            .map(|e| {
                let pubkey = PubKey::from_str(&e.pubkey)
                    .with_context(|| format!("parsing signature pubkey '{}'", e.pubkey))?;
                let cleaned = e
                    .sig
                    .strip_prefix("0x")
                    .or_else(|| e.sig.strip_prefix("0X"))
                    .unwrap_or(e.sig.as_str());
                let sig_bytes = hex::decode(cleaned)
                    .with_context(|| format!("hex-decoding signature for '{}'", e.pubkey))?;
                let sig = SafeVec::<u8, MAX_ATTESTOR_SIGNATURE_BYTES>::try_from(sig_bytes)
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "signature for '{}' exceeds {} bytes",
                            e.pubkey,
                            MAX_ATTESTOR_SIGNATURE_BYTES
                        )
                    })?;
                Ok(AttestorSignature { pubkey, sig })
            })
            .collect::<Result<Vec<_>>>()?;
        let signature_count = signatures.len();

        let private_key = resolve_signer(&self.signer)?;
        let call_message =
            ligate_client::submit_attestation::<S>(schema_id, payload_hash, signatures)
                .context("building SubmitAttestation call")?;
        let call = RuntimeCall::Attestation(call_message);

        let tx_hash = build_sign_submit(global, &self.chain, &private_key, call).await?;

        // The receipt id is the `<schemaId>:<payloadHash>` compound;
        // it's what `ligate query attestation` takes.
        let attestation_id = AttestationId::from_pair(&schema_id, &payload_hash);

        if global.json {
            let payload = SubmitAttestationJson {
                attestation_id: attestation_id.to_string(),
                schema_id: schema_id.to_string(),
                payload_hash: payload_hash.to_string(),
                signature_count,
                tx_hash,
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("Attestation submitted:");
            println!("  attestation_id: {attestation_id}");
            println!("  signatures:     {signature_count}");
            println!("  tx:             {tx_hash}");
        }
        Ok(())
    }
}
