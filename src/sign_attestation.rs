//! `ligate sign-attestation` subcommand: produce a single attestor's
//! signature over the canonical `SignedAttestationPayload` digest.
//!
//! The attestor half of the attestation flow. `submit-attestation`
//! takes a pre-aggregated JSON file of signatures and submits the
//! on-chain tx; this command produces ONE such signature entry given
//! the attestor's local key, so a quorum of attestors can each run it
//! (e.g. each on their own laptop) and the relayer concatenates the
//! outputs into a `--signatures` file before submitting.
//!
//! ## What gets signed
//!
//! The chain re-derives the [canonical digest][digest] at submission
//! time from `(schema_id, payload_hash, submitter, timestamp)` and
//! verifies each signature against it. So this command must take all
//! four inputs and produce the signature over the exact same digest.
//!
//! - `schema_id`: bech32m `lsc1...`, required.
//! - `payload_hash`: bech32m `lph1...`, required. Either pass directly
//!   via `--payload-hash` or have the cli compute it from the canonical
//!   payload bytes via `--payload-file` (SHA-256 of the file's bytes).
//! - `submitter`: bech32m `lig1...`, required. The address that will
//!   sign the on-chain submission tx (NOT the attestor's address).
//! - `timestamp`: u64 unix seconds, default 0. Pinned to 0 in chain
//!   v0 because the runtime doesn't yet expose block-header time to
//!   module handlers (chain `attestation/src/lib.rs:1557`). When the
//!   chain starts stamping real block times, this default flips.
//!
//! ## Output
//!
//! The `signatures` field is the array shape `submit-attestation
//! --signatures` consumes. With one attestor in your quorum, write it
//! to a file and pass it through. With many, run `sign-attestation`
//! on each attestor's machine and concatenate (e.g.,
//! `jq -s 'add' a.json b.json c.json > signatures.json`).
//!
//! [digest]: https://github.com/ligate-io/ligate-chain/blob/main/crates/modules/attestation/src/lib.rs

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use clap::Args;
use ed25519_dalek::SigningKey;
use ligate_client::{attestation_digest, sign_attestation, PayloadHash, SchemaId};
use ligate_rollup::MockRollupSpec;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sov_modules_api::execution_mode::Native;
use sov_modules_api::Spec;

use crate::attest::SignerArgs;
use crate::cli::GlobalArgs;
use crate::keystore::resolve_signer_key;

type S = MockRollupSpec<Native>;
type SovAddress = <S as Spec>::Address;

#[derive(Debug, Args)]
pub struct SignAttestationCmd {
    /// Bech32m `lsc1...` id of the schema this attestation is under.
    #[arg(long)]
    pub schema: String,

    /// Bech32m `lph1...` payload hash. Use this when the canonical
    /// payload bytes were hashed off-cli (e.g., by the off-chain
    /// indexer). Conflicts with `--payload-file`.
    #[arg(long, conflicts_with = "payload_file")]
    pub payload_hash: Option<String>,

    /// Path to the canonical payload bytes the attestor witnessed.
    /// The cli reads the file's bytes verbatim (no JSON
    /// re-canonicalization), SHA-256s them, and uses the result as
    /// the payload hash. Conflicts with `--payload-hash`.
    #[arg(long)]
    pub payload_file: Option<PathBuf>,

    /// Bech32m `lig1...` address that will submit the on-chain tx.
    /// NOT the attestor's address (the attestor's pubkey is derived
    /// from the signer flag below). Part of the canonical digest the
    /// chain re-derives at submission time, so the signature only
    /// validates if `submit-attestation`'s `--signer` matches.
    #[arg(long)]
    pub submitter: String,

    /// Unix seconds timestamp the digest is computed against.
    /// Defaults to 0 because chain v0 hardcodes 0 in
    /// `handle_submit_attestation` (the runtime doesn't yet expose
    /// block timestamps to module handlers; tracked in
    /// `ligate-chain/issues/TBD`). Override only if you know the
    /// chain you are signing for uses a different timestamp source.
    #[arg(long, default_value = "0")]
    pub timestamp: u64,

    /// Write the signatures array to this file instead of stdout.
    /// The shape matches what `submit-attestation --signatures`
    /// consumes, so the natural flow is `sign-attestation --output
    /// sigs.json && submit-attestation --signatures sigs.json`.
    #[arg(long)]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    pub signer: SignerArgs,
}

/// One entry in the signatures array (matches the JSON shape
/// `submit-attestation --signatures` expects).
#[derive(Serialize)]
struct SignatureEntry {
    pubkey: String,
    sig: String,
}

#[derive(Serialize)]
struct SignAttestationJson {
    payload_hash: String,
    digest: String,
    signature: SignatureEntry,
    /// Array form (single-entry), drop-in for `submit-attestation
    /// --signatures` in the 1-of-1 case. M-of-N flows concatenate
    /// these arrays across attestors before submitting.
    signatures: Vec<SignatureEntry>,
}

impl SignAttestationCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        // 1. Resolve payload hash from either form.
        let payload_hash = match (self.payload_hash.as_deref(), self.payload_file.as_ref()) {
            (Some(_), Some(_)) => {
                anyhow::bail!("pass --payload-hash or --payload-file, not both")
            }
            (None, None) => {
                anyhow::bail!("must pass --payload-hash or --payload-file")
            }
            (Some(s), None) => {
                PayloadHash::from_str(s).with_context(|| format!("parsing --payload-hash '{s}'"))?
            }
            (None, Some(p)) => {
                let bytes =
                    fs::read(p).with_context(|| format!("reading payload file {}", p.display()))?;
                let digest: [u8; 32] = Sha256::digest(&bytes).into();
                PayloadHash::from(digest)
            }
        };

        // 2. Parse the other digest inputs.
        let schema_id = SchemaId::from_str(&self.schema)
            .with_context(|| format!("parsing --schema '{}'", self.schema))?;
        let submitter = SovAddress::from_str(&self.submitter)
            .map_err(|e| anyhow::anyhow!("parsing --submitter '{}': {e:?}", self.submitter))?;

        // 3. Re-derive the canonical digest the chain will check
        //    against. Same helper the chain's on-submission verifier
        //    uses; signing against this means the chain's verifier
        //    re-derives the same bytes and the ed25519 check passes.
        let digest = attestation_digest::<S>(schema_id, payload_hash, submitter, self.timestamp);

        // 4. Resolve the attestor signing key.
        let key_hex = match (&self.signer.signer, &self.signer.private_key_hex) {
            (Some(_), Some(_)) => {
                anyhow::bail!("pass --signer or --private-key-hex, not both");
            }
            (Some(role), None) => resolve_signer_key(role, self.signer.keystore.as_deref())
                .with_context(|| format!("loading key for role '{role}'"))?,
            (None, Some(hex_seed)) => hex_seed
                .strip_prefix("0x")
                .or_else(|| hex_seed.strip_prefix("0X"))
                .unwrap_or(hex_seed)
                .to_string(),
            (None, None) => {
                anyhow::bail!("must pass --signer or --private-key-hex");
            }
        };
        let key_bytes = hex::decode(&key_hex).context("hex-decoding signer key")?;
        let seed: [u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("key seed must be 32 bytes, got {}", key_bytes.len()))?;
        let signing_key = SigningKey::from_bytes(&seed);

        // 5. Sign the digest. Returns an `AttestorSignature` with the
        //    attestor's pubkey (lpk1...) and 64-byte ed25519 sig.
        let signature = sign_attestation(&signing_key, &digest);
        let pubkey_str = signature.pubkey.to_string();
        // SafeVec impls Deref<Target=[T]>, so `&sig[..]` yields the
        // underlying slice without bringing the trait into scope.
        let sig_hex = hex::encode(&signature.sig[..]);
        let entry = SignatureEntry {
            pubkey: pubkey_str.clone(),
            sig: sig_hex.clone(),
        };

        // 6. Emit output.
        if global.json || self.output.is_some() {
            let payload = SignAttestationJson {
                payload_hash: payload_hash.to_string(),
                digest: hex::encode(digest),
                signature: SignatureEntry {
                    pubkey: pubkey_str.clone(),
                    sig: sig_hex.clone(),
                },
                signatures: vec![entry],
            };
            let body = serde_json::to_string_pretty(&payload)?;
            match &self.output {
                Some(path) => {
                    fs::write(path, format!("{body}\n"))
                        .with_context(|| format!("writing {}", path.display()))?;
                    if !global.json {
                        eprintln!("wrote signatures to {}", path.display());
                    }
                }
                None => println!("{body}"),
            }
        } else {
            println!("Attestation signed:");
            println!("  payload_hash: {payload_hash}");
            println!("  digest:       {}", hex::encode(digest));
            println!("  pubkey:       {pubkey_str}");
            println!("  sig:          {sig_hex}");
            println!();
            println!("Pass `--signatures` to `submit-attestation` with:");
            println!("  [{{\"pubkey\":\"{pubkey_str}\",\"sig\":\"{sig_hex}\"}}]");
        }
        Ok(())
    }
}
