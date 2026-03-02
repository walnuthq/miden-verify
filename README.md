# miden-verify

Verify Miden accounts from the command-line.

> [!WARNING]
> This tool is still a work in progress.

The `miden-verify` executable allows Miden smart contracts verification, give it a deployed account address and a Miden Rust project path and it will re-compile the Rust source code and verify that it matches the on-chain Miden Assembly.

## Usage

To get started, you must first install `miden-verify`:

```
cargo install miden-verify
```

> [!IMPORTANT]
> Until this crate has been published to crates.io, it is only possible to
> install using `cargo install --path .` or `cargo install --git <repo_uri>`.

### Verifying a contract

```
miden-verify mtst1azemf595fuvmzypdjg5525wwvsv39q2e_qruqqypuyph --project-path ~/miden-projects/counter-account
```
