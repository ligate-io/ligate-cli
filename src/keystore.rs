//! Re-exports the keystore-side helpers from [`crate::keys`] under a
//! more discoverable name.
//!
//! Subcommands that just need to load a key (transfer, future
//! `attest`, future `schema register`) import from here instead of
//! pulling in the full clap command tree.

pub use crate::keys::{read_address, resolve_signer_key};
// `read_key_hex` is used internally by `resolve_signer_key`; not yet
// re-exported here because no subcommand consumes the raw hex form
// directly. Re-add the re-export when the first one does.
