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

Match the style of the module you are editing.

- **Formatting is automated.** `cargo fmt` is the source of truth for style, and
  `cargo clippy --workspace --all-targets -- -D warnings` must be clean - both
  are enforced by CI.
- **Comment the why, not the what.** Prefer code that reads on its own; add a
  comment where the reasoning behind a decision isn't obvious, and keep it
  accurate as the code changes.
- **Errors are actionable.** New failures use the `LSW####` scheme (see
  `crates/lsw-core/src/error.rs`) with a message that tells the user how to fix
  the problem, not just what went wrong.
- **Never equate Wine with native Windows.** LSW's premise is honest, measured
  compatibility - keep local-runtime results distinct from native-verification
  results, and never report a Wine pass as a Windows guarantee.
- **Tests come with changes.** New behavior needs unit tests; bug fixes need a
  regression test. Keep `cargo test --workspace` green.
- **Reuse the existing abstractions.** Prefer the established patterns - the
  `RuntimeProvider` / `ToolchainProvider` traits, the error enum, the `Dirs`
  layout - over new one-off mechanisms.
- **Keep dependencies lean.** Add a crate only when it clearly earns its place;
  `cargo deny` gates licenses and advisories.
- **Write clear commit messages.** Follow
  [Conventional Commits](https://www.conventionalcommits.org) - a `feat:` /
  `fix:` / `docs:` / `chore:` / `refactor:` prefix and a subject that explains
  the *why* when it isn't obvious.

## Testing against real Windows

The native-verification paths (`lsw verify --native-windows`, `lsw compat
--native`, `lsw debug --native`, WinRM/HTTPS transports) need a reachable
Windows host configured under `[verify]` in `lsw.toml`. These are exercised
manually, not in CI. `docs/compatibility.md` describes the transports and setup.

## Scope

LSW targets Windows *userspace* application development from Linux. Kernel
drivers, a full Windows desktop, Active Directory, and redistributing
proprietary Microsoft content are explicit non-goals - please open an issue
before proposing work in those directions.

## License

By contributing you agree that your contributions are licensed under the
project's dual `Apache-2.0 OR MIT` terms.
