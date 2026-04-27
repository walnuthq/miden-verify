use anyhow::{Context, Result, bail};
use cargo_toml::Manifest;
use clap::Parser;
use miden_protocol::{
    account::AccountId,
    address::{Address, AddressId, NetworkId},
    note::NoteId,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
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
    /// Network ID (mtst = testnet, mdev = devnet)
    #[arg(long, default_value = "mtst", value_name = "NETWORK_ID")]
    network_id: String,
    /// Account address, account ID, or note ID to verify
    #[arg(value_name = "RESOURCE_ID")]
    resource_id: String,
    /// Project path containing Cargo.toml and src/lib.rs
    #[arg(long, default_value = ".", value_name = "PROJECT_PATH")]
    project_path: PathBuf,
    /// Verifier API endpoint
    #[arg(
        long,
        default_value = "https://miden-playground-api.walnut.dev",
        value_name = "VERIFIER_URL"
    )]
    verifier_url: String,
}

// --- Request / response types ---

#[derive(Debug, Serialize)]
struct PackageSource {
    #[serde(rename = "cargoToml")]
    cargo_toml: String,
    rust: String,
}

#[derive(Debug, Serialize)]
struct VerifyAccountComponentRequestBody {
    #[serde(rename = "accountId")]
    account_id: String,
    identifier: String,
    #[serde(rename = "packageSource")]
    package_source: PackageSource,
}

#[derive(Debug, Serialize)]
struct VerifyNoteRequestBody {
    #[serde(rename = "noteId")]
    note_id: String,
    #[serde(rename = "packageSource")]
    package_source: PackageSource,
    dependencies: Vec<PackageSource>,
}

#[derive(Debug, Deserialize)]
struct VerifyResponse {
    verified: bool,
}

// --- Cargo.toml metadata types for dependency resolution ---

#[derive(Deserialize)]
struct MidenDependency {
    path: String,
}

#[derive(Deserialize)]
struct MidenMetadata {
    #[serde(default)]
    dependencies: HashMap<String, MidenDependency>,
}

#[derive(Deserialize)]
struct PackageMetadata {
    miden: Option<MidenMetadata>,
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
    bail!(
        "'{}' is not a valid account address, account ID, or note ID",
        resource_id
    )
}

// --- Package source helpers ---

fn read_package_source(project_dir: &Path) -> Result<PackageSource> {
    let cargo_toml =
        fs::read_to_string(project_dir.join("Cargo.toml")).context("failed to read Cargo.toml")?;
    let rust =
        fs::read_to_string(project_dir.join("src/lib.rs")).context("failed to read src/lib.rs")?;
    Ok(PackageSource { cargo_toml, rust })
}

fn read_package_dependencies(cargo_toml: &str, project_dir: &Path) -> Result<Vec<PackageSource>> {
    let manifest = Manifest::<PackageMetadata>::from_slice_with_metadata(cargo_toml.as_bytes())
        .context("failed to parse Cargo.toml")?;
    let Some(miden) = manifest
        .package
        .and_then(|p| p.metadata)
        .and_then(|m| m.miden)
    else {
        return Ok(vec![]);
    };
    miden
        .dependencies
        .values()
        .map(|dep| read_package_source(&project_dir.join(&dep.path)))
        .collect()
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
    let VerifyResponse { verified } = response
        .json()
        .await
        .context("failed to parse verifier response")?;
    Ok(verified)
}

async fn verify_account_component(
    client: &Client,
    network_id: &NetworkId,
    account_id: &AccountId,
    project_dir: &Path,
    verifier_url: &str,
) -> Result<bool> {
    println!(
        "Verifying account {} on network {}, project: {}",
        account_id,
        network_id,
        project_dir.display()
    );
    let body = VerifyAccountComponentRequestBody {
        account_id: account_id.to_hex(),
        identifier: account_id.to_bech32(network_id.clone()),
        package_source: read_package_source(project_dir)?,
    };
    let url = format!(
        "{}/verified-account-components/{}",
        verifier_url,
        network_id.as_str()
    );
    post_verify(client, &url, &body).await
}

async fn verify_note(
    client: &Client,
    network_id: &NetworkId,
    note_id: &NoteId,
    project_dir: &Path,
    verifier_url: &str,
) -> Result<bool> {
    println!(
        "Verifying note {} on network {}, project: {}",
        note_id,
        network_id,
        project_dir.display()
    );
    let package_source = read_package_source(project_dir)?;
    let dependencies = read_package_dependencies(&package_source.cargo_toml, project_dir)?;
    let body = VerifyNoteRequestBody {
        note_id: note_id.to_hex(),
        package_source,
        dependencies,
    };
    let url = format!("{}/verified-notes/{}", verifier_url, network_id.as_str());
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
