//! Print the canonical `SignedAttestationPayload` digest for a
//! hard-coded test vector, plus the borsh bytes that get SHA-256'd.
//!
//! Mirrors the test vector in the chain repo's
//! `docs/protocol/attestation-v0.md` §wire-format. Use this when
//! you're rolling an off-chain signer in another language and want
//! to cross-check your borsh layout: re-run the same inputs in your
//! language, dump the bytes, compare against this output. Any drift
//! between the two (most commonly the missing `0x00` MultiAddress
//! discriminator before the submitter) shows up as a different
//! digest at the same input.
//!
//! Run with:
//!
//! ```sh
//! cargo run --release --example derive_digest
//! ```

use std::str::FromStr;

use ligate_client::{attestation_digest, PayloadHash, SchemaId, SignedAttestationPayload};
use ligate_rollup::MockRollupSpec;
use sov_modules_api::execution_mode::Native;
use sov_modules_api::Spec;

type S = MockRollupSpec<Native>;

fn main() -> anyhow::Result<()> {
    let schema_id =
        SchemaId::from_str("lsc1rsj2sjurqllj4859jgvdwerkjvj4tdfpf7xpc42jyjmzp793jjrqu3an2p")?;
    let payload_hash =
        PayloadHash::from_str("lph1he42sgwn7mp5qhkuklxahn6pncqpr83jr3hmgezj9qw6l42ezwkqw5zknm")?;
    let submitter =
        <S as Spec>::Address::from_str("lig1zd9j2z6x55ydnv9m8f0pdw3vs2j8u0w5sdqeaf478dzp6s998ac")
            .map_err(|e| anyhow::anyhow!("addr parse: {e:?}"))?;
    let timestamp = 0u64;

    let payload = SignedAttestationPayload::<S> {
        schema_id,
        payload_hash,
        submitter,
        timestamp,
    };
    let bytes = borsh::to_vec(&payload)?;
    let digest = attestation_digest::<S>(schema_id, payload_hash, submitter, timestamp);

    println!("inputs:");
    println!(
        "  schema_id    {schema_id} = {}",
        hex::encode(schema_id.as_bytes())
    );
    println!(
        "  payload_hash {payload_hash} = {}",
        hex::encode(payload_hash.as_bytes())
    );
    println!(
        "  submitter    {submitter} = {}",
        hex::encode(submitter.as_ref())
    );
    println!("  timestamp    {timestamp}");
    println!();
    println!("borsh bytes  ({} bytes)", bytes.len());
    println!("  {}", hex::encode(&bytes));
    println!();
    println!("digest       sha256(borsh) = {}", hex::encode(digest));
    Ok(())
}
