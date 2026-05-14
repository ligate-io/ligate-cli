//! `ligate register-attestor-set` subcommand: register a quorum of
//! attestor public keys + an M-of-N threshold.
//!
//! An attestor set is the "who can sign" half of an attestation
//! schema. Register one here, get back the deterministic `las1...`
//! id, then point a schema at it via `ligate register-schema`.
//!
//! The set id is `SHA-256(borsh(members) ‖ threshold)`; the chain
//! re-derives it on submission, so callers don't supply it. We compute
//! it locally too, purely so the command can echo it back.

use std::str::FromStr;

use anyhow::{Context, Result};
use clap::Args;
use ligate_client::{AttestorSet, PubKey};
use ligate_stf::runtime::RuntimeCall;
use serde::Serialize;

use crate::attest::{build_sign_submit, resolve_signer, ChainArgs, SignerArgs, S};
use crate::cli::GlobalArgs;

#[derive(Debug, Args)]
pub struct RegisterAttestorSetCmd {
    /// Comma-separated attestor public keys, each bech32m `lpk1...`.
    /// Order is not significant: the chain stores members sorted and
    /// the set id is order-independent.
    #[arg(long, value_delimiter = ',', required = true)]
    pub members: Vec<String>,

    /// M-of-N threshold: how many distinct members must co-sign an
    /// attestation for it to be accepted. Must be `1..=members.len()`.
    #[arg(long)]
    pub threshold: u8,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[command(flatten)]
    pub chain: ChainArgs,
}

#[derive(Serialize)]
struct RegisterAttestorSetJson {
    attestor_set_id: String,
    threshold: u8,
    members: Vec<String>,
    tx_hash: String,
}

impl RegisterAttestorSetCmd {
    pub async fn run(self, global: &GlobalArgs) -> Result<()> {
        if self.threshold == 0 {
            anyhow::bail!("--threshold must be at least 1");
        }

        // Parse bech32m `lpk1...` member keys.
        let members: Vec<PubKey> = self
            .members
            .iter()
            .map(|s| PubKey::from_str(s).with_context(|| format!("parsing member pubkey '{s}'")))
            .collect::<Result<Vec<_>>>()?;

        if usize::from(self.threshold) > members.len() {
            anyhow::bail!(
                "--threshold {} exceeds the {} member(s) supplied",
                self.threshold,
                members.len()
            );
        }

        // Deterministic id the chain will re-derive on submission;
        // computed here only so we can echo it back to the operator.
        let attestor_set_id = AttestorSet::derive_id(&members, self.threshold);

        let private_key = resolve_signer(&self.signer)?;
        let call_message = ligate_client::register_attestor_set::<S>(members, self.threshold)
            .context("building RegisterAttestorSet call")?;
        let call = RuntimeCall::Attestation(call_message);

        let tx_hash = build_sign_submit(global, &self.chain, &private_key, call).await?;

        if global.json {
            let payload = RegisterAttestorSetJson {
                attestor_set_id: attestor_set_id.to_string(),
                threshold: self.threshold,
                members: self.members.clone(),
                tx_hash,
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            println!("Attestor set registered:");
            println!("  attestor_set_id: {attestor_set_id}");
            println!(
                "  threshold:       {} of {}",
                self.threshold,
                self.members.len()
            );
            println!("  tx:              {tx_hash}");
        }
        Ok(())
    }
}
