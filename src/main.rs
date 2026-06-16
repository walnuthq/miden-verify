use anyhow::{Context, Result, bail};
use clap::Parser;
use miden_protocol::{
    account::AccountId,
    address::{Address, AddressId, NetworkId},
    note::NoteId,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// CLI to verify Miden accounts & notes
#[derive(Parser, Debug)]
#[command(
    name = "miden-verify",
    version,
    about = "Verify Miden accounts & notes",
    long_about = None
)]
struct Args {
    /// Account address, account ID, or note ID to verify
    #[arg(value_name = "RESOURCE_ID")]
    resource_id: String,
    /// Network ID (mtst = testnet, mdev = devnet)
    #[arg(long, default_value = "mtst", value_name = "NETWORK_ID")]
    network_id: String,
    /// Top-level project directory containing the entrypoint package and its dependencies
    #[arg(long, default_value = ".", value_name = "PROJECT_PATH")]
    project_path: PathBuf,
    /// Entrypoint package (relative to PROJECT_PATH) identifying the main package
    #[arg(long, default_value = ".", value_name = "ENTRYPOINT")]
    entrypoint: String,
    /// Verifier API endpoint
    #[arg(
        long,
        default_value = "https://miden-source-code-verification-api-registry.walnut.dev",
        value_name = "VERIFIER_URL"
    )]
    verifier_url: String,
}

// --- Request / response types ---

#[derive(Debug, Serialize)]
struct VerifyAccountRequestBody {
    #[serde(rename = "accountId")]
    account_id: String,
    files: BTreeMap<String, String>,
    entrypoint: String,
}

#[derive(Debug, Serialize)]
struct VerifyNoteRequestBody {
    #[serde(rename = "noteId")]
    note_id: String,
    files: BTreeMap<String, String>,
    entrypoint: String,
}

#[derive(Debug, Deserialize)]
struct VerifyResponse {
    verified: bool,
}

// --- Resource parsing ---

enum Resource {
    Account {
        network_id: Option<NetworkId>,
        account_id: AccountId,
    },
    Note(NoteId),
}

fn parse_resource_id(resource_id: &str) -> Result<Resource> {
    if let Ok((account_id, network_id)) = AccountId::parse(resource_id) {
        return Ok(Resource::Account {
            network_id,
            account_id,
        });
    }
    if let Ok((network_id, address)) = Address::decode(resource_id) {
        let AddressId::AccountId(account_id) = address.id() else {
            bail!("address '{}' does not contain an account ID", resource_id);
        };
        return Ok(Resource::Account {
            network_id: Some(network_id),
            account_id,
        });
    }
    if let Ok(note_id) = NoteId::try_from_hex(resource_id) {
        return Ok(Resource::Note(note_id));
    }
    bail!("'{}' is not a valid account address, account ID, or note ID", resource_id)
}

// --- Project file collection ---

/// Returns the relative path rendered as a `/`-joined key, or `None` if any
/// component is not valid UTF-8.
fn relative_key(rel: &Path) -> Option<String> {
    let components =
        rel.components().map(|c| c.as_os_str().to_str()).collect::<Option<Vec<_>>>()?;
    Some(components.join("/"))
}

/// Decides whether a project-relative file path should be uploaded.
///
/// Included: `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, and any
/// file living under a `src/` directory (recursively). Everything else
/// (notably `Cargo.lock`, build artifacts) is excluded.
fn is_included(rel: &Path) -> bool {
    let components = rel.components().filter_map(|c| c.as_os_str().to_str()).collect::<Vec<_>>();
    let Some((file_name, parents)) = components.split_last() else {
        return false;
    };
    // Any file under a `src/` directory (covers nested modules).
    if parents.contains(&"src") {
        return true;
    }
    if *file_name == "Cargo.toml" || *file_name == "rust-toolchain.toml" {
        return true;
    }
    // `.cargo/config.toml`
    if *file_name == "config.toml" && parents.last() == Some(&".cargo") {
        return true;
    }
    false
}

fn collect_files(dir: &Path, base: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if file_type.is_dir() {
            // Skip build artifacts and hidden directories other than `.cargo`.
            if name == "target" || (name.starts_with('.') && name != ".cargo") {
                continue;
            }
            collect_files(&path, base, files)?;
        } else if file_type.is_file() {
            let rel = path.strip_prefix(base).expect("walked path must be under base");
            if is_included(rel) {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                if let Some(key) = relative_key(rel) {
                    files.insert(key, content);
                }
            }
        }
    }
    Ok(())
}

/// Walks `project_dir` and builds the map of project-relative file paths to
/// their UTF-8 contents that the verifier API expects in its `files` field.
fn build_files_map(project_dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    collect_files(project_dir, project_dir, &mut files)?;
    Ok(files)
}

// --- Verification ---

async fn post_verify<B: Serialize + ?Sized>(client: &Client, url: &str, body: &B) -> Result<bool> {
    let response = client
        .post(url)
        .json(body)
        .send()
        .await
        .context("failed to send verification request")?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        bail!("verifier returned {}: {}", status, text);
    }
    let VerifyResponse { verified } =
        response.json().await.context("failed to parse verifier response")?;
    Ok(verified)
}

async fn verify_account_component(
    client: &Client,
    network_id: &NetworkId,
    account_id: &AccountId,
    project_dir: &Path,
    entrypoint: &str,
    verifier_url: &str,
) -> Result<bool> {
    println!(
        "Verifying account {} on network {}, project: {}, entrypoint: {}",
        account_id,
        network_id,
        project_dir.display(),
        entrypoint
    );
    let body = VerifyAccountRequestBody {
        account_id: account_id.to_hex(),
        files: build_files_map(project_dir)?,
        entrypoint: entrypoint.to_string(),
    };
    let url = format!("{}/v1/{}/verified-accounts", verifier_url, network_id.as_str());
    post_verify(client, &url, &body).await
}

async fn verify_note(
    client: &Client,
    network_id: &NetworkId,
    note_id: &NoteId,
    project_dir: &Path,
    entrypoint: &str,
    verifier_url: &str,
) -> Result<bool> {
    println!(
        "Verifying note {} on network {}, project: {}, entrypoint: {}",
        note_id,
        network_id,
        project_dir.display(),
        entrypoint
    );
    let body = VerifyNoteRequestBody {
        note_id: note_id.to_hex(),
        files: build_files_map(project_dir)?,
        entrypoint: entrypoint.to_string(),
    };
    let url = format!("{}/v1/{}/verified-notes", verifier_url, network_id.as_str());
    post_verify(client, &url, &body).await
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    let args = Args::parse();

    let fallback_network_id = NetworkId::new(&args.network_id).context("invalid --network-id")?;

    let project_dir = args.project_path.as_path();
    if !project_dir.is_dir() {
        bail!("'{}' is not a directory", project_dir.display());
    }

    let client = Client::new();

    let (verified, kind) = match parse_resource_id(&args.resource_id)? {
        Resource::Account {
            network_id,
            account_id,
        } => {
            let network_id = network_id.unwrap_or(fallback_network_id);
            let verified = verify_account_component(
                &client,
                &network_id,
                &account_id,
                project_dir,
                &args.entrypoint,
                &args.verifier_url,
            )
            .await?;
            (verified, "Account component")
        }
        Resource::Note(note_id) => {
            let verified = verify_note(
                &client,
                &fallback_network_id,
                &note_id,
                project_dir,
                &args.entrypoint,
                &args.verifier_url,
            )
            .await?;
            (verified, "Note script")
        }
    };

    if verified {
        println!("{} successfully verified", kind);
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("{} could not be verified", kind);
        Ok(ExitCode::FAILURE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("project-template")
    }

    #[test]
    fn single_package_root() {
        let dir = template_dir().join("counter-account");
        let files = build_files_map(&dir).expect("build_files_map");

        let keys: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![".cargo/config.toml", "Cargo.toml", "rust-toolchain.toml", "src/lib.rs"],
            "unexpected file set for single-package project_path"
        );

        // Excluded files must not appear.
        assert!(!files.contains_key("Cargo.lock"));
        assert!(!files.contains_key(".DS_Store"));

        // Contents are read verbatim from disk.
        let expected = fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert_eq!(files["src/lib.rs"], expected);
        assert!(!files["Cargo.toml"].is_empty());
    }

    #[test]
    fn multi_package_root() {
        let dir = template_dir();
        let files = build_files_map(&dir).expect("build_files_map");

        for pkg in ["counter-account", "increment-note"] {
            for suffix in ["Cargo.toml", "rust-toolchain.toml", ".cargo/config.toml", "src/lib.rs"]
            {
                let key = format!("{pkg}/{suffix}");
                assert!(files.contains_key(&key), "missing expected key {key}");
            }
        }

        // No Cargo.lock from either package should be uploaded.
        assert!(files.keys().all(|k| !k.ends_with("Cargo.lock")), "Cargo.lock leaked into files");
    }

    #[test]
    fn excludes_artifacts_and_hidden_files() {
        let files = build_files_map(&template_dir()).expect("build_files_map");
        assert!(
            files.keys().all(|k| !k.split('/').any(|c| c == "target")),
            "target/ artifacts must be excluded"
        );
        assert!(files.keys().all(|k| !k.ends_with(".DS_Store")), ".DS_Store must be excluded");
    }

    /// End-to-end check against a locally running verifier.
    ///
    /// Ignored by default because it requires a verifier listening on
    /// `http://localhost:8081`. Run it explicitly with:
    /// `cargo test verifies_account_against_local_verifier -- --ignored`
    #[tokio::test]
    #[ignore = "requires a local verifier running at http://localhost:8081"]
    async fn verifies_account_against_local_verifier() {
        let project_dir = template_dir().join("counter-account");
        let network_id = NetworkId::new("mtst").expect("network id");

        let Resource::Account { account_id, .. } =
            parse_resource_id("0x2a4ffb87b51720105c3bf91e5e7bd8").expect("parse resource id")
        else {
            panic!("expected an account resource");
        };

        let verified = verify_account_component(
            &Client::new(),
            &network_id,
            &account_id,
            &project_dir,
            ".",
            "http://localhost:8081",
        )
        .await
        .expect("verification request");

        assert!(verified, "account should be verified by the local verifier");
    }

    /// End-to-end check of note verification against a locally running verifier.
    ///
    /// Ignored by default because it requires a verifier listening on
    /// `http://localhost:8081`. Run it explicitly with:
    /// `cargo test verifies_note_against_local_verifier -- --ignored`
    #[tokio::test]
    #[ignore = "requires a local verifier running at http://localhost:8081"]
    async fn verifies_note_against_local_verifier() {
        let project_dir = template_dir();
        let network_id = NetworkId::new("mtst").expect("network id");

        let Resource::Note(note_id) =
            parse_resource_id("0x44891875fb920d963352fcd6623e1f3c97dd1e4d8cdc084778eeb4bbdf72dbac")
                .expect("parse resource id")
        else {
            panic!("expected a note resource");
        };

        let verified = verify_note(
            &Client::new(),
            &network_id,
            &note_id,
            &project_dir,
            "increment-note",
            "http://localhost:8081",
        )
        .await
        .expect("verification request");

        assert!(verified, "note should be verified by the local verifier");
    }
}
