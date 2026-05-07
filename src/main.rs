//! `ligate` CLI entrypoint.
//!
//! Subcommand dispatch lives in [`cli`]. This file is just argument
//! parsing, tracing setup, and the top-level error formatter.

mod balance;
mod cli;
mod config;
mod faucet;
mod keys;
mod keystore;
mod transfer;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Command};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    // Init tracing before any other call so errors during dispatch
    // get structured output. Default to `warn` so the CLI is quiet
    // by default; bump with `RUST_LOG=ligate=info`.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .without_time()
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Command::Keys(cmd) => cmd.run().await,
        Command::Balance(cmd) => cmd.run(&cli.global).await,
        Command::Transfer(cmd) => cmd.run(&cli.global).await,
        Command::Faucet(cmd) => cmd.run(&cli.global).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Print the error chain on stderr. `{:#}` walks the
            // anyhow chain.
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
