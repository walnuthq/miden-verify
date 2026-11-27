use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use miden_protocol::{
    account::AccountId,
    address::{Address, AddressId, NetworkId},
    note::NoteId,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

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
    /// Network ID (mtst = testnet, mdev = devnet, mm = mainnet)
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

/// Identifier of the client originating verification requests, recorded on the
/// verified account/note by the registry.
const SOURCE: &str = "miden-verify";

#[derive(Debug, Serialize)]
struct VerifyAccountRequestBody {
    #[serde(rename = "accountId")]
    account_id: String,
    files: BTreeMap<String, String>,
    entrypoint: String,
    source: String,
}

#[derive(Debug, Serialize)]
struct VerifyNoteRequestBody {
    #[serde(rename = "noteId")]
    note_id: String,
    files: BTreeMap<String, String>,
    entrypoint: String,
    source: String,
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

const MANIFEST: &str = "miden-project.toml";

/// Returns the relative path rendered as a `/`-joined key, or `None` if any
/// component is not valid UTF-8.
fn relative_key(rel: &Path) -> Option<String> {
    let components =
        rel.components().map(|c| c.as_os_str().to_str()).collect::<Option<Vec<_>>>()?;
    Some(components.join("/"))
}

/// Decides whether a project-relative file path should be uploaded: every
/// `miden-project.toml` and every `*.masm` file, wherever it sits, since a
/// manifest's `[lib] path` can point anywhere in the tree. Directories are
/// pruned in [`collect_files`].
///
/// Keep in step with the web-verifier's `collect-project-files.ts` in
/// `walnuthq/miden-source-code-verification`.
fn is_included(rel: &Path) -> bool {
    let Some(file_name) = rel.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name == MANIFEST || file_name.ends_with(".masm")
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
            // Skip build artifacts and hidden directories.
            if name == "target" || name.starts_with('.') {
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

/// Builds the files map for `project_dir` and checks, before any request, that
/// `<project_dir>/<entrypoint>` is a MASM project the verifier can take.
fn collect_project(project_dir: &Path, entrypoint: &str) -> Result<BTreeMap<String, String>> {
    let files = build_files_map(project_dir)?;
    let package_dir = match entrypoint {
        "." | "" => project_dir.to_path_buf(),
        _ => project_dir.join(entrypoint),
    };
    let has_masm = files.keys().any(|key| key.ends_with(".masm"));
    if package_dir.join("Cargo.toml").is_file() && !has_masm {
        bail!(
            "{} is a Rust project: Rust projects are no longer supported, see the README",
            package_dir.display()
        );
    }
    if !package_dir.join(MANIFEST).is_file() {
        bail!("no {} found in {}", MANIFEST, package_dir.display());
    }
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
        files: collect_project(project_dir, entrypoint)?,
        entrypoint: entrypoint.to_string(),
        source: SOURCE.to_string(),
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
        files: collect_project(project_dir, entrypoint)?,
        entrypoint: entrypoint.to_string(),
        source: SOURCE.to_string(),
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

    /// Temporary project tree, removed on drop.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let root =
                std::env::temp_dir().join(format!("miden-verify-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            for (rel, content) in files {
                let path = root.join(rel);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, content).unwrap();
            }
            Self(root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn single_package_root() {
        let dir = template_dir().join("counter-contract");
        let files = build_files_map(&dir).expect("build_files_map");

        let keys: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["counter_contract.masm", "miden-project.toml"],
            "unexpected file set for single-package project_path"
        );

        // Contents are read verbatim from disk.
        let expected = fs::read_to_string(dir.join("counter_contract.masm")).unwrap();
        assert_eq!(files["counter_contract.masm"], expected);
        assert!(!files["miden-project.toml"].is_empty());
    }

    #[test]
    fn multi_package_root() {
        let dir = template_dir();
        let files = build_files_map(&dir).expect("build_files_map");

        let keys: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "counter-contract/counter_contract.masm",
                "counter-contract/miden-project.toml",
                "counter-note/counter_note.masm",
                "counter-note/miden-project.toml",
            ],
            "unexpected file set for multi-package project_path"
        );
        collect_project(&dir, "counter-note").expect("counter-note is a MASM project");
    }

    #[test]
    fn includes_masm_anywhere_and_excludes_rust_artifacts_and_hidden_files() {
        let fixture = Fixture::new(
            "collect",
            &[
                ("miden-project.toml", "[package]"),
                ("lib.masm", "# root"),
                ("asm/nested/util.masm", "# nested"),
                ("src/main.masm", "# src"),
                ("Cargo.toml", "[package]"),
                ("Cargo.lock", "version = 4"),
                ("build.rs", "fn main() {}"),
                ("rust-toolchain.toml", "[toolchain]"),
                ("src/lib.rs", "// lib"),
                (".cargo/config.toml", "[build]"),
                (".hidden/hidden.masm", "# hidden"),
                ("README.md", "# readme"),
                (".DS_Store", ""),
                ("target/out/miden-project.toml", "[package]"),
                ("target/out/lib.masm", "# built"),
            ],
        );
        let files = build_files_map(&fixture.0).expect("build_files_map");

        let keys: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["asm/nested/util.masm", "lib.masm", "miden-project.toml", "src/main.masm"],
            "unexpected file set"
        );
        assert_eq!(files["asm/nested/util.masm"], "# nested");
    }

    #[test]
    fn refuses_entrypoint_without_manifest() {
        let fixture = Fixture::new(
            "no-manifest",
            &[("pkg/miden-project.toml", "[package]"), ("pkg/lib.masm", "# lib")],
        );
        let err = collect_project(&fixture.0, ".").expect_err("no manifest at the root");
        assert!(err.to_string().contains("no miden-project.toml found in"), "{err}");
        collect_project(&fixture.0, "pkg").expect("manifest under the entrypoint");
    }

    #[test]
    fn refuses_rust_project() {
        // A project from the Rust compiler era: a manifest, but no MASM sources.
        let fixture = Fixture::new(
            "rust",
            &[
                ("Cargo.toml", "[package]"),
                ("miden-project.toml", "[package]"),
                ("src/lib.rs", "// lib"),
            ],
        );
        let err = collect_project(&fixture.0, ".").expect_err("Rust project");
        assert!(err.to_string().contains("Rust projects are no longer supported"), "{err}");

        // Without a manifest it is still reported as a Rust project.
        let fixture = Fixture::new("rust-no-manifest", &[("Cargo.toml", "[package]")]);
        let err = collect_project(&fixture.0, ".").expect_err("Rust project");
        assert!(err.to_string().contains("Rust projects are no longer supported"), "{err}");
    }

    /// Unwraps a verification result, counting the registry's "already verified"
    /// rejection as a success: the registry only raises it once the compiled
    /// package has matched on-chain, and it is what every run after the first
    /// one gets back, since a successful verification is recorded.
    fn verified_or_already_verified(result: Result<bool>, already_verified: &str) -> bool {
        match result {
            Ok(verified) => verified,
            Err(err) if err.to_string().contains(already_verified) => true,
            Err(err) => panic!("verification request: {err:#}"),
        }
    }

    /// End-to-end check against a locally running verifier.
    ///
    /// Ignored by default because it requires a verifier listening on
    /// `http://localhost:8081`. Run it explicitly with:
    /// `cargo test verifies_account_against_local_verifier -- --ignored`
    #[tokio::test]
    #[ignore = "requires a local verifier running at http://localhost:8081"]
    async fn verifies_account_against_local_verifier() {
        let project_dir = template_dir().join("counter-contract");
        let network_id = NetworkId::new("mtst").expect("network id");

        let Resource::Account { account_id, .. } =
            parse_resource_id("0x2d719c50f6bfa3113cb6960d5ba6d7").expect("parse resource id")
        else {
            panic!("expected an account resource");
        };

        let result = verify_account_component(
            &Client::new(),
            &network_id,
            &account_id,
            &project_dir,
            ".",
            "http://localhost:8081",
        )
        .await;
        let verified = verified_or_already_verified(result, "account component already verified");

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
            parse_resource_id("0xa1eebb1da617de82a9b3fdc8d7151c2e1161ea9f76834981d8914fa97086685f")
                .expect("parse resource id")
        else {
            panic!("expected a note resource");
        };

        let result = verify_note(
            &Client::new(),
            &network_id,
            &note_id,
            &project_dir,
            "counter-note",
            "http://localhost:8081",
        )
        .await;
        let verified = verified_or_already_verified(result, "note already verified");

        assert!(verified, "note should be verified by the local verifier");
    }
}
