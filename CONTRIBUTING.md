# Contributing to LSW

Thanks for your interest. LSW is a Rust workspace; contributions go through pull
requests against `main`.

## Development setup

Building and testing LSW needs only a stable Rust toolchain (see
`rust-toolchain.toml`; MSRV is 1.85). Wine, MinGW-w64, and the other Windows
tooling are *runtime* dependencies for using `lsw`, not for building it, so the
test suite runs offline on a stock Linux install.

```
git clone https://github.com/johnqherman/LSW
cd LSW
cargo build --workspace
```

## The gate

Every change must pass the same checks CI runs. Run them before opening a PR:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check          # optional locally; CI runs it
```

## Conventions

This repo is deliberately spare. Match the surrounding code:

- **No comments in source.** Code should read on its own. The one exception is
  the clap `///` help text in `crates/lsw-cli/src/main.rs`, which is functional
  (it generates `--help`).
- **ASCII only.** No smart quotes, em/en dashes, or other non-ASCII bytes.
- **Errors are actionable.** New error variants use the `LSW####` scheme with a
  message that says how to fix the problem (see `crates/lsw-core/src/error.rs`).
- **Title-only commits.** One-line, imperative subjects
  (`feat: ...`, `fix: ...`, `docs: ...`); no body unless the *why* is
  non-obvious.

## Testing against real Windows

The native-verification paths (`lsw verify --native-windows`, `lsw compat
--native`, `lsw debug --native`, WinRM/HTTPS transports) need a reachable
Windows host configured under `[verify]` in `lsw.toml`. These are exercised
manually, not in CI. `docs/compatibility.md` describes the transports and setup.

## Scope

LSW targets Windows *userspace* application development from Linux. Kernel
drivers, a full Windows desktop, Active Directory, and redistributing
proprietary Microsoft content are explicit non-goals - please open a discussion
before proposing work in those directions.

## License

By contributing you agree that your contributions are licensed under the
project's dual `Apache-2.0 OR MIT` terms.
