//! `ligate keys` subcommand: local Ed25519 keystore management.
//!
//! v0 surface: `generate`, `list`, `show`. Each role gets two files:
//!
//! - `<role>.key`     hex-encoded 32-byte private key, mode 0600
//! - `<role>.address` `lig1...` bech32m address (plaintext + newline)
//!
//! The address derivation is `Address = SHA-256(pubkey)[..28]`, identical
//! to the chain's `ligate-genesis-tool keys generate`. Keystores
//! produced by either tool are interoperable.
//!
//! Future v1 work tracked in chain repo
//! [#112](https://github.com/ligate-io/ligate-chain/issues/112):
//! `import --from-mnemonic` for Mneme interop, encrypted-at-rest
//! files, hardware-wallet support.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sov_modules_api::Address;

/// Default keystore root: `$XDG_DATA_HOME/ligate/keys` on Linux,
/// `~/Library/Application Support/io.ligate.cli/keys` on macOS.
fn default_keystore_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("io", "ligate", "cli")
        .context("could not resolve OS-default project dirs")?;
    Ok(dirs.data_dir().join("keys"))
}

#[derive(Debug, Subcommand)]
pub enum KeysCmd {
    /// Generate a new Ed25519 keypair, write `<role>.key` + `<role>.address`.
    Generate {
        /// Role label. Used as the filename stem and as the lookup key
        /// for `--signer` flags on other commands.
        #[arg(long)]
        name: String,

        /// Output directory. Defaults to the OS keystore dir.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// List all roles in the keystore.
    List {
        /// Keystore directory. Defaults to the OS keystore dir.
        #[arg(long)]
        keystore: Option<PathBuf>,
    },

    /// Show the address for one role.
    Show {
        /// Role label.
        name: String,

        /// Keystore directory. Defaults to the OS keystore dir.
        #[arg(long)]
        keystore: Option<PathBuf>,
    },
}

impl KeysCmd {
    pub async fn run(self) -> Result<()> {
        match self {
            Self::Generate { name, output } => {
                let dir = match output {
                    Some(p) => p,
                    None => default_keystore_dir()?,
                };
                let g = generate_role(&name, &dir)?;
                println!("Generated key for role '{}':", g.role);
                println!("  address: {}", g.address);
                println!("  key:     {}", g.key_path.display());
                println!("  (mode 0600, do not commit)");
                Ok(())
            }
            Self::List { keystore } => {
                let dir = match keystore {
                    Some(p) => p,
                    None => default_keystore_dir()?,
                };
                if !dir.exists() {
                    println!("(no keystore at {})", dir.display());
                    return Ok(());
                }
                let mut roles = list_roles(&dir)?;
                roles.sort();
                if roles.is_empty() {
                    println!("(empty keystore at {})", dir.display());
                    return Ok(());
                }
                for role in roles {
                    let addr = read_address(&dir, &role).unwrap_or_else(|_| "<missing>".into());
                    println!("{role:20} {addr}");
                }
                Ok(())
            }
            Self::Show { name, keystore } => {
                let dir = match keystore {
                    Some(p) => p,
                    None => default_keystore_dir()?,
                };
                let addr = read_address(&dir, &name)
                    .with_context(|| format!("no key for role '{name}' in {}", dir.display()))?;
                println!("{addr}");
                Ok(())
            }
        }
    }
}

/// Result of generating one role-tagged keypair.
///
/// `address_path` is part of the public surface even though no
/// consumer reads it yet. Useful for future callers that want to
/// verify file placement (e.g., scripts that lift `generate_role` for
/// fixture setup), and matches the chain genesis-tool's struct shape
/// so downstream code can pin to a single layout.
#[allow(dead_code)]
pub struct GeneratedKey {
    pub role: String,
    pub address: String,
    pub key_path: PathBuf,
    pub address_path: PathBuf,
}

/// Generate one Ed25519 keypair, derive its `lig1...` address, and
/// persist both to disk.
///
/// The output dir is created if it doesn't exist. The private-key
/// file is written with mode `0600` so only the running operator
/// can read it. Mirrors the chain's `genesis-tool/src/keys.rs`
/// byte-for-byte so keystores are interchangeable.
pub fn generate_role(role: &str, output_dir: &Path) -> Result<GeneratedKey> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("creating output dir {}", output_dir.display()))?;

    // Sample 32 bytes of CSPRNG entropy. Avoids needing the
    // `rand_core` feature on ed25519-dalek (which adds a
    // version-coupling we'd rather not pull through transitively).
    let mut secret_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut secret_bytes);
    let signing_key = SigningKey::from_bytes(&secret_bytes);
    let pubkey_bytes = signing_key.verifying_key().to_bytes();

    // Address derivation: SHA-256(pubkey)[..28].
    let digest = Sha256::digest(pubkey_bytes);
    let mut addr_bytes = [0u8; 28];
    addr_bytes.copy_from_slice(&digest[..28]);
    let address = Address::from(addr_bytes);
    let address_str = address.to_string();

    let key_path = output_dir.join(format!("{role}.key"));
    let address_path = output_dir.join(format!("{role}.address"));

    // Belt-and-braces 0600: open(2) with mode 0600, then re-apply
    // via set_permissions to cover the case where the file already
    // existed and `truncate(true)` rewrote it.
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&key_path)
            .with_context(|| format!("creating {}", key_path.display()))?;
        let key_hex = hex::encode(signing_key.to_bytes());
        f.write_all(key_hex.as_bytes())?;
        f.write_all(b"\n")?;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
    }

    fs::write(&address_path, format!("{address_str}\n"))
        .with_context(|| format!("writing {}", address_path.display()))?;

    Ok(GeneratedKey {
        role: role.to_string(),
        address: address_str,
        key_path,
        address_path,
    })
}

/// Read all role names that have a `.key` file in the given keystore.
fn list_roles(keystore: &Path) -> Result<Vec<String>> {
    let mut roles = Vec::new();
    for entry in fs::read_dir(keystore)
        .with_context(|| format!("reading keystore {}", keystore.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "key") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                roles.push(stem.to_string());
            }
        }
    }
    Ok(roles)
}

/// Read `<role>.address` from the keystore.
pub fn read_address(keystore: &Path, role: &str) -> Result<String> {
    let path = keystore.join(format!("{role}.address"));
    let s = fs::read_to_string(&path)
        .with_context(|| format!("reading address file {}", path.display()))?;
    Ok(s.trim_end_matches('\n').to_string())
}

/// Read `<role>.key` from the keystore as a 64-char hex string.
pub fn read_key_hex(keystore: &Path, role: &str) -> Result<String> {
    let path = keystore.join(format!("{role}.key"));
    let s = fs::read_to_string(&path)
        .with_context(|| format!("reading key file {}", path.display()))?;
    let s = s.trim_end_matches('\n').to_string();
    if s.len() != 64 {
        bail!(
            "key file {} has {} chars, expected 64 hex chars",
            path.display(),
            s.len()
        );
    }
    Ok(s)
}

/// Resolve a `--signer NAME` (or `--keystore PATH`) flag to the hex
/// private key for use by chain-submitting subcommands.
pub fn resolve_signer_key(role: &str, keystore: Option<&Path>) -> Result<String> {
    let dir = match keystore {
        Some(p) => p.to_path_buf(),
        None => default_keystore_dir()?,
    };
    read_key_hex(&dir, role)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generates_lig1_address() {
        let dir = tempdir().unwrap();
        let g = generate_role("operator", dir.path()).unwrap();
        assert!(g.address.starts_with("lig1"), "got {}", g.address);

        let key_bytes = fs::read(&g.key_path).unwrap();
        assert_eq!(key_bytes.len(), 65, "key file should be 64 hex + newline");
        let mode = fs::metadata(&g.key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "key file should be chmod 600, got {mode:o}");
    }

    #[test]
    fn list_and_show_roundtrip() {
        let dir = tempdir().unwrap();
        let _ = generate_role("alice", dir.path()).unwrap();
        let _ = generate_role("bob", dir.path()).unwrap();
        let mut roles = list_roles(dir.path()).unwrap();
        roles.sort();
        assert_eq!(roles, vec!["alice".to_string(), "bob".to_string()]);

        let a = read_address(dir.path(), "alice").unwrap();
        assert!(a.starts_with("lig1"));
    }

    #[test]
    fn missing_role_errors() {
        let dir = tempdir().unwrap();
        let err = read_address(dir.path(), "ghost").unwrap_err();
        assert!(err.to_string().contains("address file"));
    }

    #[test]
    fn key_hex_roundtrip_succeeds() {
        let dir = tempdir().unwrap();
        let g = generate_role("hexcheck", dir.path()).unwrap();
        let hex = read_key_hex(dir.path(), "hexcheck").unwrap();
        assert_eq!(hex.len(), 64);
        // Address derivation should match what we wrote.
        assert!(g.address.starts_with("lig1"));
    }
}
