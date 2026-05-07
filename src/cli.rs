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
    /// Defaults to the public devnet RPC. Set to `http://127.0.0.1:12346`
    /// for a local node, or override per-command for one-off targets.
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
