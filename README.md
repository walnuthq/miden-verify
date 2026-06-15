# miden-verify

Verify Miden accounts and notes from the command-line.

> [!WARNING]
> This tool is still a work in progress.

The `miden-verify` CLI allows Miden accounts and notes verification, give it a deployed account address or note ID and a Miden Rust project path and it will re-compile the Rust source code and verify that it matches the on-chain Miden Assembly.

## Usage

To get started, you must first install [midenup](https://github.com/0xMiden/midenup).

### Verifying a contract

```
miden verify mtst1azg2fhnwnx3jsqzmdcf6y5ec6ce7dymx --project-path ~/miden/project-template/contracts/counter-account
```

### Verifying a note with dependencies

When the project path contains several packages (the entrypoint package and its dependencies), point `--project-path` at the top-level directory and select the entrypoint package with `--endpoint`:

```
miden verify 0x... --project-path ~/miden/project-template --endpoint increment-note
```

### Options

- `<RESOURCE_ID>` Account address, account ID or note ID (required).
- `--network-id <NETWORK_ID>` Network ID (mtst/mdev, required when not decoded from account address, defaults to `mtst`).
- `--project-path <PROJECT_PATH>` Top-level project directory containing the entrypoint package and its dependencies (defaults to the current working directory). The CLI uploads `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml` and `src/` files found in the tree.
- `--endpoint <ENDPOINT>` Entrypoint package relative to `PROJECT_PATH` identifying the main package (defaults to `.`).
- `--verifier-url <VERIFIER_URL>` Verification API endpoint (defaults to `https://miden-sourcify-api-registry.walnut.dev`).

## License

MIT
