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

### Options

- `<RESOURCE_ID>` Account address, account ID or note ID (required).
- `[NETWORK_ID]` Network ID (mtst/mdev, required when not decoded from account address).
- `[PROJECT_PATH]` Project path containing `Cargo.toml` and `src/lib.rs` (defaults to current working directory).
- `[VERIFIER_URL]` Verification API endpoint (optional).

## License

MIT
