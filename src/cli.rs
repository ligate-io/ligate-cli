//! Top-level clap command tree for `ligate`.
//!
//! Subcommand modules each provide their own arg struct and `run`
//! method; this file just stitches them together so the binary
//! discovers them via a single `match`.

use clap::{Args, Parser, Subcommand};

use crate::balance::BalanceCmd;
use crate::faucet::FaucetCmd;
use crate::keys::KeysCmd;
use crate::transfer::TransferCmd;

/// Operator + builder CLI for Ligate Chain.
#[derive(Debug, Parser)]
#[command(
    name = "ligate",
    version,
    about,
    long_about = None,
    propagate_version = true,
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

/// Args common to every subcommand that talks to the chain.
///
/// Lives at the top level so subcommands don't have to re-declare
/// `--rpc` / `--json` / etc. Subcommands take `&GlobalArgs` in their
/// `run` method.
#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// RPC endpoint of a Ligate Chain node.
    ///
    /// Pass either `https://rpc.ligate.io` or a local node like
    /// `http://127.0.0.1:12346`. The cli appends the chain's `/v1`
    /// API prefix internally before talking to the node, so you do
    /// not need to include it here. (If you do, the cli detects it
    /// and won't double-add.)
    #[arg(
        long,
        global = true,
        env = "LIGATE_RPC",
        default_value = "https://rpc.ligate.io"
    )]
    pub rpc: String,

    /// Emit JSON output instead of human-readable text.
    ///
    /// Useful for piping into `jq` or for scripted operator workflows.
    /// Per-subcommand the JSON shape is documented inline.
    #[arg(long, global = true)]
    pub json: bool,
}

impl GlobalArgs {
    /// Returns the RPC base URL with the chain's `/v1` API prefix
    /// guaranteed to be present. Idempotent.
    ///
    /// The Sovereign SDK's `NodeClient::new(base)` probes
    /// `{base}/modules` (unprefixed). Our chain mounts at `/v1/...`,
    /// so the base passed to the SDK must end in `/v1` for the probe
    /// and every subsequent state query to land. This helper
    /// centralises the normalisation so subcommand code stays
    /// simple. See unit tests for the input/output mapping.
    pub fn rpc_with_v1(&self) -> String {
        let trimmed = self.rpc.trim_end_matches('/');
        if trimmed.ends_with("/v1") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/v1")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> GlobalArgs {
        Cli::parse_from(args).global
    }

    #[test]
    fn appends_v1_to_default_rpc() {
        // The default RPC is `https://rpc.ligate.io` (no /v1).
        // We need a subcommand to make clap happy, so use `keys list`.
        let g = parse(&["ligate", "keys", "list"]);
        assert_eq!(g.rpc_with_v1(), "https://rpc.ligate.io/v1");
    }

    #[test]
    fn appends_v1_to_local_rpc_no_trailing_slash() {
        let g = parse(&["ligate", "--rpc", "http://127.0.0.1:12346", "keys", "list"]);
        assert_eq!(g.rpc_with_v1(), "http://127.0.0.1:12346/v1");
    }

    #[test]
    fn appends_v1_to_local_rpc_with_trailing_slash() {
        let g = parse(&["ligate", "--rpc", "http://127.0.0.1:12346/", "keys", "list"]);
        assert_eq!(g.rpc_with_v1(), "http://127.0.0.1:12346/v1");
    }

    #[test]
    fn does_not_double_prefix_when_v1_already_present() {
        let g = parse(&[
            "ligate",
            "--rpc",
            "http://127.0.0.1:12346/v1",
            "keys",
            "list",
        ]);
        assert_eq!(g.rpc_with_v1(), "http://127.0.0.1:12346/v1");
    }

    #[test]
    fn strips_trailing_slash_after_v1() {
        let g = parse(&[
            "ligate",
            "--rpc",
            "http://127.0.0.1:12346/v1/",
            "keys",
            "list",
        ]);
        assert_eq!(g.rpc_with_v1(), "http://127.0.0.1:12346/v1");
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage local Ed25519 keypairs.
    #[command(subcommand)]
    Keys(KeysCmd),

    /// Read the `$LGT` balance of an address.
    Balance(BalanceCmd),

    /// Send `$LGT` from a local key to another address.
    Transfer(TransferCmd),

    /// Claim a one-shot drip from the devnet faucet.
    Faucet(FaucetCmd),
}
