//! Shared chain-identity config for chain-submitting subcommands.
//!
//! Transfer + future attest/schema/attestor-set submit transactions
//! that bind to a specific chain. Operator passes the IDs explicitly
//! per command, or via env vars on the host. Defaults match
//! `ligate-devnet-1` once those constants are pinned (#237 in
//! ligate-chain); for now they stay required.
//!
//! ## Identifier formats
//!
//! Both `chain_hash` and `lgt_token_id` are 32-byte values. Since
//! `ligate-chain@0ac7e5b` the chain emits them as bech32m strings
//! (`lsch1...` and `token_1...` respectively) on every partner-visible
//! surface: `GET /v1/rollup/info`, the explorer, the chain's own
//! `ligate-node` output. Earlier chain revs emitted raw hex. The
//! parsers in this module accept three forms each:
//!
//! - bech32m with the type's HRP (`lsch1...` or `token_1...`)
//! - hex with a leading `0x` or `0X`
//! - bare 64-char hex (legacy, retained so captured fixtures and
//!   older operator scripts keep working)
//!
//! This lets `export LIGATE_CHAIN_HASH=$(ligate info --json | jq -r .chain_hash)`
//! work without a manual conversion step regardless of which chain rev
//! the operator's node is pinned at.
//!
//! Wrong-HRP bech32m and length mismatches are rejected loudly.

use anyhow::{Context, Result};
use sov_bank::TokenId;

/// HRP for the build-time chain-hash fingerprint.
const CHAIN_HASH_HRP: &str = "lsch";

/// HRP for token identifiers (sov-bank's [`TokenId`] type).
///
/// Underscore-terminated by convention in the SDK's macro (see
/// `impl_hash32_type!(TokenId, TokenIdBech32, "token_")` in
/// `sov-bank/src/token.rs`); the separator the bech32m crate inserts
/// is `1`, so a token id reads `token_1...`. The HRP we hand
/// `bech32::decode` for prefix-checking is the underscore-terminated
/// `"token_"` literal.
const TOKEN_ID_HRP: &str = "token_";

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

/// Parse a 32-byte chain hash. Accepts:
///
/// - bech32m `lsch1...` (canonical since `ligate-chain@0ac7e5b`;
///   pass the string from `ligate info --json | jq -r .chain_hash`
///   directly)
/// - hex with a leading `0x` / `0X`
/// - bare 64-char hex (legacy)
pub fn parse_chain_hash(s: &str) -> Result<[u8; 32]> {
    parse_32_byte_id(s, CHAIN_HASH_HRP).context("parsing chain hash")
}

/// Parse a [`TokenId`]. Accepts:
///
/// - bech32m `token_1...` (canonical since `ligate-chain@0ac7e5b`;
///   matches the explorer + node output)
/// - hex with a leading `0x` / `0X`
/// - bare 64-char hex (legacy)
pub fn parse_token_id(s: &str) -> Result<TokenId> {
    let bytes = parse_32_byte_id(s, TOKEN_ID_HRP).context("parsing token id")?;
    Ok(TokenId::from(bytes))
}

/// Shared parser for any 32-byte identifier that ligate-chain emits.
///
/// The HRP argument is used both to decide which decode path to take
/// (bech32m vs hex) and to validate the bech32m prefix when the input
/// matches the bech32m shape. Wrong-HRP bech32m inputs error rather
/// than silently falling through to the hex path so a `token_1...`
/// passed to `--chain-hash` fails loudly with a clear message.
fn parse_32_byte_id(s: &str, expected_hrp: &str) -> Result<[u8; 32]> {
    // bech32m: HRP + '1' + 5-bit data + 6-char checksum. We sniff for
    // `<hrp>1` so a `token_1...` going through `--chain-hash` (HRP
    // `lsch`) doesn't silently match. Real `lsch1...` strings always
    // start with the `<expected_hrp>1` prefix because bech32m's
    // separator is fixed.
    let hrp_prefix = format!("{expected_hrp}1");
    let bytes: Vec<u8> = if s.starts_with(&hrp_prefix) {
        let (hrp, data) = bech32::decode(s).context("bech32m decode")?;
        if hrp.as_str() != expected_hrp {
            anyhow::bail!(
                "expected bech32m HRP '{}', got '{}'",
                expected_hrp,
                hrp.as_str()
            );
        }
        data
    } else if let Some(stripped) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if stripped.len() != 64 {
            anyhow::bail!(
                "expected 64 hex chars after '0x' prefix, got {}",
                stripped.len()
            );
        }
        hex::decode(stripped).context("hex decode")?
    } else {
        if s.len() != 64 {
            anyhow::bail!(
                "expected bech32m `{}1...`, `0x`-prefixed hex, or 64 bare hex chars; \
                 got {} chars with no recognised prefix",
                expected_hrp,
                s.len()
            );
        }
        hex::decode(s).context("hex decode")?
    };

    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|v: Vec<u8>| anyhow::anyhow!("expected 32-byte payload, got {} bytes", v.len()))?;
    Ok(arr)
}

/// Bech32m-encode a 32-byte chain hash as `lsch1...`.
///
/// Mirrors the chain's `LschHash::Display` impl. Reserved for the
/// future case where a subcommand wants to re-emit a chain hash it
/// took in via `--chain-hash` (which accepts hex too) back through
/// the canonical bech32m form. `info` doesn't need it because the
/// chain's JSON already comes back `lsch1...` since
/// `ligate-chain@0ac7e5b`; kept here as the API-parity counterpart
/// to [`encode_token_id`] so future commands have a single place to
/// reach for.
#[allow(dead_code)]
pub fn encode_chain_hash(bytes: &[u8; 32]) -> String {
    encode_32_byte_id(bytes, CHAIN_HASH_HRP)
}

/// Bech32m-encode a [`TokenId`] as `token_1...`.
///
/// Mirrors the chain's `TokenIdBech32::Display` impl from sov-bank.
pub fn encode_token_id(token_id: &TokenId) -> String {
    encode_32_byte_id(token_id.as_bytes(), TOKEN_ID_HRP)
}

fn encode_32_byte_id(bytes: &[u8], hrp_str: &str) -> String {
    let hrp = bech32::Hrp::parse(hrp_str).expect("hrp literal must be valid");
    bech32::encode::<bech32::Bech32m>(hrp, bytes).expect("bech32m encode never fails for [u8;32]")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO_BYTES: [u8; 32] = [0u8; 32];

    fn fixture_bytes() -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, b) in out.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(3);
        }
        out
    }

    #[test]
    fn chain_hash_bare_hex_roundtrip() {
        let h = "00".repeat(32);
        let bytes = parse_chain_hash(&h).unwrap();
        assert_eq!(bytes, ZERO_BYTES);
    }

    #[test]
    fn chain_hash_prefixed_hex_roundtrip() {
        let h = format!("0x{}", "00".repeat(32));
        let bytes = parse_chain_hash(&h).unwrap();
        assert_eq!(bytes, ZERO_BYTES);
    }

    #[test]
    fn chain_hash_bech32m_roundtrip() {
        let bytes = fixture_bytes();
        let s = encode_chain_hash(&bytes);
        assert!(s.starts_with("lsch1"), "got {s}");
        let decoded = parse_chain_hash(&s).unwrap();
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn chain_hash_rejects_short_bare_hex() {
        assert!(parse_chain_hash("dead").is_err());
    }

    #[test]
    fn chain_hash_rejects_wrong_hrp_bech32m() {
        // Encode something with the token HRP, try to parse as chain hash.
        let bytes = fixture_bytes();
        let token_str = encode_32_byte_id(&bytes, TOKEN_ID_HRP);
        // The parser detects this doesn't start with `lsch1` so it
        // falls into the hex path; assert it errors rather than
        // silently succeeding (the hex decode will fail loudly).
        assert!(parse_chain_hash(&token_str).is_err());
    }

    #[test]
    fn token_id_bare_hex_roundtrip() {
        let h = "00".repeat(32);
        let id = parse_token_id(&h).unwrap();
        assert_eq!(id.as_bytes(), &ZERO_BYTES);
    }

    #[test]
    fn token_id_bech32m_roundtrip() {
        let bytes = fixture_bytes();
        let s = encode_32_byte_id(&bytes, TOKEN_ID_HRP);
        assert!(s.starts_with("token_1"), "got {s}");
        let id = parse_token_id(&s).unwrap();
        assert_eq!(id.as_bytes(), &bytes);
    }

    #[test]
    fn encode_chain_hash_starts_with_lsch1() {
        let s = encode_chain_hash(&ZERO_BYTES);
        assert!(s.starts_with("lsch1"), "got {s}");
    }

    #[test]
    fn encode_token_id_starts_with_token1() {
        let s = encode_token_id(&TokenId::from(ZERO_BYTES));
        assert!(s.starts_with("token_1"), "got {s}");
    }
}
