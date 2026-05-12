//! `ligate completions <SHELL>` subcommand.
//!
//! Emits a `clap_complete`-generated completion script to stdout for
//! the requested shell. Operators redirect to the standard install
//! path for their shell, then tab-completion works on `ligate`.
//!
//! Install snippets (also documented in the README):
//!
//! ```sh
//! # zsh (user-level)
//! ligate completions zsh > ~/.local/share/zsh/site-functions/_ligate
//!
//! # bash (system-wide)
//! ligate completions bash | sudo tee /etc/bash_completion.d/ligate
//!
//! # fish (user-level)
//! ligate completions fish > ~/.config/fish/completions/ligate.fish
//!
//! # powershell — append to your $PROFILE
//! ligate completions powershell >> $PROFILE
//! ```
//!
//! ## Why this lives in the CLI binary (not a separate `xtask`)
//!
//! `clap_complete::generate` needs the live `clap::Command` tree —
//! the same one [`crate::cli::Cli`] derives. Building it in-process
//! keeps the completion script in sync with the actual subcommands
//! without a duplicate command-graph that drifts.

use std::io;

use anyhow::Result;
use clap::{Args, CommandFactory};
use clap_complete::{generate, Shell};

use crate::cli::Cli;

#[derive(Debug, Args)]
pub struct CompletionsCmd {
    /// Target shell. One of `bash`, `zsh`, `fish`, `elvish`,
    /// `powershell`.
    pub shell: Shell,
}

impl CompletionsCmd {
    pub async fn run(self) -> Result<()> {
        // Generate against the live Cli command tree. `Cli::command()`
        // comes from the `CommandFactory` derive on `#[derive(Parser)]`.
        let mut cmd = Cli::command();
        let bin_name = cmd.get_name().to_string();
        generate(self.shell, &mut cmd, bin_name, &mut io::stdout());
        Ok(())
    }
}
