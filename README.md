# miden-verify

Verify Miden accounts and notes from the command-line.

> [!WARNING]
> This tool is still a work in progress.

The `miden-verify` CLI allows Miden accounts and notes verification, give it a deployed account address or note ID and a Miden Rust project path and it will re-compile the Rust source code and verify that it matches the on-chain Miden Assembly.

## Usage

To get started, you must first install [midenup](https://github.com/0xMiden/midenup).

### Verifying a contract

Pass the account ID (or address) of the deployed account:

```
miden verify 0x2a4ffb87b51720105c3bf91e5e7bd8 --project-path ~/miden-verify/project-template/counter-account
```

You can also pass a bech32 account address, which carries its own network ID:

```
miden verify mtst1aq4yl7u8k5tjqyzu80u3uhnmmqam87dz --project-path ~/miden-verify/project-template/counter-account
```

### Verifying a note with dependencies

When the project path contains several packages (the entrypoint package and its dependencies), point `--project-path` at the top-level directory and select the entrypoint package with `--entrypoint`:

```
miden verify 0x44891875fb920d963352fcd6623e1f3c97dd1e4d8cdc084778eeb4bbdf72dbac --project-path ~/miden-verify/project-template --entrypoint increment-note
```

### Options

- `<RESOURCE_ID>` Account address, account ID or note ID (required).
- `--network-id <NETWORK_ID>` Network ID (mtst/mdev, required when not decoded from account address, defaults to `mtst`).
- `--project-path <PROJECT_PATH>` Top-level project directory containing the entrypoint package and its dependencies (defaults to the current working directory). The CLI uploads `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml` and `src/` files found in the tree.
- `--entrypoint <ENTRYPOINT>` Entrypoint package relative to `PROJECT_PATH` identifying the main package (defaults to `.`).
- `--verifier-url <VERIFIER_URL>` Verification API endpoint (defaults to `https://miden-source-code-verification-api-registry.walnut.dev`).

## License

MIT
