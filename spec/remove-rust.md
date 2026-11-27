# Remove Rust project support from `miden-verify`

Status: implemented on `feat/masm-projects` (2026-11-27); release steps not done. Written 2026-11-25.

> **Executor**: this plan is written for one agent in one session and ends in one PR. Start from an updated `main`, on the branch `feat/masm-projects`. Commit there; push, open a PR, publish a release or open a PR in another repo only when the user's prompt says so. No Claude attribution in commits or PRs. Deploying the template to Testnet needs the user's go-ahead in your session. Before starting, ask the user to confirm that the verifier has dropped cargo (its plans 08 and 09, `spec/remove-rust/` in `walnuthq/miden-source-code-verification`) and read the file rules its web-verifier shipped, since this CLI's filter must match them. Verify with `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test --all-targets --locked`, then the two ignored tests against a local verifier. Stop and report if the verifier's request or response shape differs from "Request and response" below.

## Why

The Miden Rust compiler (`cargo-miden`, `midenc`) is being phased out. Accounts and notes are written in plain MASM again, as `miden-project.toml` projects. The verifier (`walnuthq/miden-source-code-verification`) and the Playground (`walnuthq/miden-playground`) are dropping Rust; the full analysis and the decisions are in the Playground repo, `spec/remove-rust.md`. This CLI uploads a project to the verifier, so it has to upload MASM projects instead of Rust ones.

The CLI itself stays a Rust program. Only what it uploads, documents and ships as a template changes.

## When

This is the last step of the rollout (step 8 in the Playground spec), decided on 2026-11-25. It starts once the verifier has dropped cargo (step 6) and has MASM fixtures on Testnet.

Two consequences of going last:

- **Until this ships, the released CLI (0.7.0) is the only one.** From the verifier's step 6 it uploads Rust projects that the registry refuses. The CLI already prints the registry's answer (`verifier returned 400: …`), so the registry's refusal should say that Rust projects are no longer verified and name the CLI version to upgrade to. That message is the verifier's work, at step 6.
- **0.7.0 can already verify some MASM projects** once the verifier accepts them (step 3): it uploads `miden-project.toml` and every file under a `src/` directory. A MASM project that keeps its sources in `src/` and has no `Cargo.toml` goes through. A project with sources elsewhere does not. Not something to document, but it explains reports from the gap.

## What changes

### File filter (`src/main.rs`, `is_included`, `collect_files`)

Today it uploads `Cargo.toml`, `Cargo.lock`, `build.rs`, `miden-project.toml`, `rust-toolchain.toml`, `.cargo/config.toml` and every file under a `src/` directory, and skips `target/` and hidden directories other than `.cargo`.

After:

- Upload `miden-project.toml` and every `*.masm` file, wherever it sits in the tree. A manifest's `[lib] path` can point anywhere, so the `src/` rule goes.
- Skip `target/` and every hidden directory. The `.cargo` exception goes.
- Nothing else is uploaded: no `Cargo.*`, `build.rs`, `rust-toolchain.toml`, `*.rs`.

This is the same filter as the web-verifier's `collect-project-files.ts`, which is a port of this one. Keep the two in step: change this one to match what the verifier shipped at step 3, not the other way round.

### Checks before the request

The CLI sends whatever it finds today and lets the verifier answer. Two cheap local checks give a better error than a round trip:

- No `miden-project.toml` at `<PROJECT_PATH>/<ENTRYPOINT>`: fail with "no miden-project.toml found in …".
- A `Cargo.toml` next to that manifest and no `*.masm` file collected: fail with "Rust projects are no longer supported, see the README".

### Request and response

`POST /v1/{network}/verified-accounts` and `/verified-notes` with `{ accountId | noteId, files, entrypoint, source }`, answer `{ verified }`. The Playground spec expects this shape to survive. Check it against the verifier as deployed after step 6 before releasing, in particular whether `entrypoint` is still a directory relative to the project path.

### Dependencies

- `miden-protocol` is pinned at `=0.17.1`, used only to parse account ids, addresses, note ids and network ids. Bump it to the version of the network at the time (0.18.x today).
- While there: `--network-id` documents `mtst` and `mdev` only. The registry also serves Mainnet (`mm`). Add it to the help text and the README if `NetworkId::new("mm")` parses on the bumped crate.

### `project-template/`

Two Rust projects today, `counter-contract` and `counter-note`, each with `Cargo.toml`, `Cargo.lock`, `build.rs`, `rust-toolchain.toml`, `.cargo/config.toml` and `src/lib.rs`.

Replace both with MASM projects of the same names: a `miden-project.toml` and one `.masm` file each, the note depending on the contract by path. The verifier's spike has working sources to start from (`spike/masm-assembly/projects/counter-account` and `increment-note` in that repo, branch `spike/masm-assembly`), and the verifier's own step 6 fixtures may be the same projects. Prefer copying the verifier's fixtures as shipped, so the README's ids are ones the verifier already tests.

The template has to be deployed on Testnet for the README and the two ignored tests to work: one account built from `counter-contract`, one note built from `counter-note`. If the verifier's fixtures are reused, their ids are reused too and nothing is deployed from here. Otherwise this is a Testnet deploy, to be confirmed with the maintainer before it is done.

### Tests (`src/main.rs`)

- `single_package_root`, `multi_package_root`: new expected key sets (`miden-project.toml` plus the `.masm` file per package).
- `includes_lockfile_and_excludes_artifacts_and_hidden_files`: rename; the fixture gains `.masm` files outside `src/`, and asserts that `Cargo.toml`, `Cargo.lock`, `build.rs`, `src/lib.rs` and `.cargo/config.toml` are not uploaded.
- New tests for the two local checks.
- The two ignored end-to-end tests (`verifies_account_against_local_verifier`, `verifies_note_against_local_verifier`) get the new account id and note id. Run both against a local verifier before the release.

### README

- The intro says the CLI "will re-compile the Rust source code": it re-assembles the MASM project.
- Usage examples: new account id, bech32 address and note id.
- `--project-path`: the new list of uploaded files.
- The midenup install line stays.

### Housekeeping

- `.gitignore`: drop `build/` ("Generated by build.rs"), which only the Rust templates produced. The Cargo entries stay, for the CLI's own build.
- `rust-toolchain.toml`, `rustfmt.toml`, `Cargo.lock` at the root belong to the CLI and stay.
- Crate `description` and `keywords` need no change.

## Release

1. Bump the crate to `0.8.0` (the filter change breaks Rust projects, and the crate is pre-1.0).
2. Publish a GitHub Release tagged `v0.8.0`. `.github/workflows/release.yml` checks the tag against the crate version, runs the tests, publishes to crates.io through trusted publishing and uploads the pre-built binaries. Do not rename the workflow file or the `release` environment.
3. Update the `miden-verify` entry in midenup's channel manifest (`0xMiden/midenup`), since `miden verify` is installed through it. That is a PR in a repo we do not own.
4. Tell the verifier side the released version, so the registry's refusal message for Rust uploads can name it.

## Not in scope

- Building or checking the project locally. The CLI stays an uploader.
- A Rust path behind a flag. The verifier will not accept Rust after step 6.
- Path dependencies that point outside `--project-path`. As today, the user points `--project-path` at the directory that holds the entrypoint and its dependencies.

## Open questions

- Does the verifier pin dependency versions for a MASM project? A Rust project carried a `Cargo.lock`; a MASM manifest says `miden-protocol = "*"` and the verifier resolves it from the packages in its image. If the verifier adds a lock or a version field to the upload, the filter follows.
- Whether the template reuses the verifier's Testnet fixtures or deploys its own (see above).

## State of this clone

On 2026-11-25 the clone is on `feat/update-to-0.17`, whose remote branch is gone because it was merged (`origin/main` is at the merge of PR #6). Local `main` is one merge behind `origin/main`. Start the work from an updated `main`.
