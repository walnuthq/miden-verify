# miden-verify

Verify Miden accounts and notes from the command-line.

> [!WARNING]
> This tool is still a work in progress.

The `miden-verify` CLI allows Miden accounts and notes verification, give it a deployed account address or note ID and a Miden project path (a MASM project with a `miden-project.toml`) and it will re-assemble the project and verify that it matches the on-chain Miden Assembly.

## Usage

To get started, you must first install [midenup](https://github.com/0xMiden/midenup).

### Verifying a contract

Pass the account ID (or address) of the deployed account:

```
miden verify 0x2d719c50f6bfa3113cb6960d5ba6d7 --project-path ~/miden-verify/project-template/counter-contract
```

You can also pass a bech32 account address, which carries its own network ID:

```
miden verify mtst1aqkhr8zs76l6xyfuk6tq6kax6uuts82p --project-path ~/miden-verify/project-template/counter-contract
```

### Verifying a note with dependencies

When the project path contains several packages (the entrypoint package and its dependencies), point `--project-path` at the top-level directory and select the entrypoint package with `--entrypoint`:

```
miden verify 0xa1eebb1da617de82a9b3fdc8d7151c2e1161ea9f76834981d8914fa97086685f --project-path ~/miden-verify/project-template --entrypoint counter-note
```

### Rust projects

Projects written in Rust for the Miden compiler (`cargo-miden`) are no longer supported: the verifier only re-assembles MASM projects. `miden-verify` 0.7.0 is the last version that uploaded Rust projects, and the verifier no longer accepts them.

### Options

- `<RESOURCE_ID>` Account address, account ID or note ID (required).
- `--network-id <NETWORK_ID>` Network ID (mtst/mdev/mm, required when not decoded from account address, defaults to `mtst`).
- `--project-path <PROJECT_PATH>` Top-level project directory containing the entrypoint package and its dependencies (defaults to the current working directory). The CLI uploads every `miden-project.toml` and `*.masm` file found in the tree (`target/` and hidden directories are skipped).
- `--entrypoint <ENTRYPOINT>` Entrypoint package relative to `PROJECT_PATH` identifying the main package (defaults to `.`).
- `--verifier-url <VERIFIER_URL>` Verification API endpoint (defaults to `https://miden-source-code-verification-api-registry.walnut.dev`).

## License

MIT
