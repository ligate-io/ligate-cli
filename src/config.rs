//! Shared chain-identity config for chain-submitting subcommands.
//!
//! Transfer + future attest/schema/attestor-set submit transactions
//! that bind to a specific chain. Operator passes the IDs explicitly
//! per command, or via env vars on the host. Defaults match
//! `ligate-devnet-1` once those constants are pinned (#237 in
//! ligate-chain); for now they stay required.

use anyhow::{Context, Result};
use sov_bank::TokenId;

/// Chain-identity inputs every state-changing command needs.
///
/// Equivalent to the `FAUCET_CHAIN_*` env vars in the faucet repo.
/// Currently each subcommand parses its own flags into the underlying
/// scalars rather than threading this through; kept here as the
/// canonical shape so future subcommands (`attest`, `schema`,
/// `attestor-set`) have a single place to construct from.
#[allow(dead_code)]
pub struct ChainIdentity {
    pub chain_id: u64,
    pub chain_hash: [u8; 32],
    pub lgt_token_id: TokenId,
}

/// Parse a 64-char hex string into a 32-byte chain hash.
pub fn parse_chain_hash(s: &str) -> Result<[u8; 32]> {
    if s.len() != 64 {
        anyhow::bail!("chain hash must be 64 hex chars, got {}", s.len());
    }
    let v = hex::decode(s).context("decoding chain hash hex")?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    Ok(out)
}

/// Parse a 64-char hex string into a 32-byte token id.
pub fn parse_token_id(s: &str) -> Result<TokenId> {
    if s.len() != 64 {
        anyhow::bail!("token id must be 64 hex chars, got {}", s.len());
    }
    let v = hex::decode(s).context("decoding token id hex")?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    Ok(TokenId::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chain_hash_roundtrip() {
        let h = "00".repeat(32);
        let bytes = parse_chain_hash(&h).unwrap();
        assert_eq!(bytes, [0u8; 32]);
    }

    #[test]
    fn parse_chain_hash_rejects_short() {
        assert!(parse_chain_hash("dead").is_err());
    }

    #[test]
    fn parse_token_id_roundtrip() {
        let h = "00".repeat(32);
        let _ = parse_token_id(&h).unwrap();
    }
}
