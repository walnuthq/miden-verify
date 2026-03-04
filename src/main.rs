use anyhow::{Context, Result};
use clap::Parser;
use miden_protocol::{account::AccountId, address::Address, address::AddressId};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

/// CLI to verify Miden accounts
#[derive(Parser, Debug)]
#[command(
    name = "miden-verify",
    version,
    about = "Verify Miden accounts",
    long_about = None
)]
struct Args {
    /// The contract account address
    #[arg(value_name = "ACCOUNT_ADDRESS")]
    account_address: String,
    // /// The network id (mtst = testnet, mdev = devnet)
    // #[arg(long, default_value = "mtst", value_name = "NETWORK_ID")]
    // network_id: String,
    /// The project path (where the contract project lives)
    #[arg(long, default_value = ".", value_name = "PROJECT_PATH")]
    project_path: PathBuf,
    /// The verifier URL (API endpoint responsible for verifying)
    #[arg(
        long,
        default_value = "https://miden-playground-api.walnut.dev/verified-account-components",
        value_name = "VERIFIER_URL"
    )]
    verifier_url: String,
}

#[derive(Debug, Serialize)]
struct VerifyAccountComponentRequestBody {
    #[serde(rename = "accountId")]
    account_id: String,
    identifier: String,
    #[serde(rename = "cargoToml")]
    cargo_toml: String,
    rust: String,
}

#[derive(Debug, Deserialize)]
struct VerifyAccountComponentRequestResponse {
    ok: bool,
    #[serde(default)]
    verified: Option<bool>,
    #[serde(default)]
    error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let (account_id, network_id) = match AccountId::parse(&args.account_address) {
        Ok((account_id, network_id)) => (account_id, network_id.unwrap()),
        Err(_) => {
            let (network_id, address) = match Address::decode(&args.account_address) {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("Failed to parse account ID: {}", error);
                    std::process::exit(1);
                }
            };
            let decoded_account_id = match address.id() {
                AddressId::AccountId(id) => id,
                _ => {
                    eprintln!("Failed to parse account ID");
                    std::process::exit(1);
                }
            };
            (decoded_account_id, network_id)
        }
    };

    println!(
        "Verifying account ID {} on network {}, project path: {}",
        account_id,
        network_id,
        args.project_path.display()
    );

    let project_dir = args.project_path.as_path();
    // Check that the directory exists
    if !project_dir.is_dir() {
        eprintln!("Error: '{}' is not a directory", project_dir.display());
        std::process::exit(1);
    }
    // Read Cargo.toml
    let cargo_path = project_dir.join("Cargo.toml");
    if !cargo_path.is_file() {
        eprintln!("Error: Cargo.toml not found in {}", project_dir.display());
        std::process::exit(1);
    }
    let cargo_toml = fs::read_to_string(&cargo_path).context("Failed to read Cargo.toml")?;
    // Read src/lib.rs
    let lib_path = project_dir.join("src").join("lib.rs");
    if !lib_path.is_file() {
        eprintln!("Error: src/lib.rs not found in {}", project_dir.display());
        std::process::exit(1);
    }
    let rust = fs::read_to_string(&lib_path).context("Failed to read src/lib.rs")?;

    let body = VerifyAccountComponentRequestBody {
        account_id: account_id.to_hex(),
        identifier: account_id.to_bech32(network_id.clone()),
        cargo_toml,
        rust,
    };
    let client = Client::builder().build()?;
    let response = client
        .post(format!("{}/{}", args.verifier_url, network_id.as_str()))
        .json(&body)
        .send()
        .await?;

    let text = response.text().await?;
    match serde_json::from_str::<VerifyAccountComponentRequestResponse>(&text) {
        Ok(response) => {
            if response.ok {
                let verified = response.verified.unwrap();
                if verified {
                    println!("Account component successfully verified.");
                } else {
                    println!("Account component not verified.");
                }
            } else {
                if let Some(error) = response.error {
                    eprintln!("Account component verification error: {}", error);
                    std::process::exit(1);
                }
            }
        }
        Err(error) => {
            eprintln!("Failed to parse JSON response: {}", error);
            std::process::exit(1);
        }
    }

    Ok(())
}
