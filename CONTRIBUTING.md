# Contributing to LSW

Thank you for your interest. LSW is a Rust workspace. Send contributions as
pull requests against `main`.

## Development setup

To build and test LSW, you need only a stable Rust toolchain (see
`rust-toolchain.toml`; the MSRV is 1.85). Wine, MinGW-w64, and the other
Windows tools are runtime dependencies of `lsw`. They are not build
dependencies. Thus the test suite runs offline on a standard Linux
installation.

```
git clone https://github.com/johnqherman/LSW
cd LSW
cargo build --workspace
```

## The gate

Each change must pass the same checks that CI runs. Run them before you open a
pull request:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check          # optional locally; CI runs it
```

CI also runs an e2e corpus (`crates/lsw-core/tests/e2e.rs`) that exercises the
real pipeline - scaffold, Wine prefix, cross-compile, run, package, MSI
install-verify - across the console/gui/dll templates plus Cargo, .NET, and
NativeAOT builds. It self-skips unless `LSW_TEST_E2E=1` is set and the needed
tools (wine, cmake, mingw-w64, and per-test extras like wixl, dotnet, clang,
lld) are on PATH:

```
LSW_TEST_E2E=1 cargo test -p lsw-core --test e2e -- --test-threads=1
```

## Conventions

Use the style of the module that you change.

- **Formatting is automated.** `cargo fmt` is the source of truth for style.
  `cargo clippy --workspace --all-targets -- -D warnings` must be clean. CI
  makes sure of both.
- **Give the why in comments, not the what.** Write code that is clear without
  comments. Add a comment where the reason for a decision is not obvious. Keep
  the comment correct when the code changes.
- **Make errors actionable.** New failures use the `LSW####` scheme (see
  `crates/lsw-core/src/error.rs`). The message must tell the user how to
  repair the problem, not only what occurred.
- **Do not tell the user that Wine is native Windows.** The premise of LSW is
  honest, measured compatibility. Keep local-runtime results and
  native-verification results separate. Do not report a Wine pass as a Windows
  guarantee.
- **Send tests with changes.** New behavior needs unit tests. Bug repairs need
  a regression test. Keep `cargo test --workspace` green.
- **Use the existing abstractions.** Use the established patterns: the
  `RuntimeProvider` and `ToolchainProvider` traits, the error enum, and the
  `Dirs` layout. Do not add new one-off mechanisms.
- **Keep the dependencies few.** Add a crate only when its value is clear.
  `cargo deny` controls the licenses and the advisories.
- **Write clear commit messages.** Use
  [Conventional Commits](https://www.conventionalcommits.org): a `feat:`,
  `fix:`, `docs:`, `chore:`, or `refactor:` prefix, and a subject that gives
  the why when the why is not obvious.

## Tests against real Windows

The native-verification paths (`lsw verify --native-windows`,
`lsw compat --native`, `lsw debug --native`, and the WinRM/HTTPS transports)
need a Windows host that you can get access to. Configure the host in
`[verify]` in `lsw.toml`. These paths are tested manually, not in CI. The
native-verification section of the README describes the transports and the
setup.

## Scope

The target of LSW is Windows userspace application development from Linux.
Kernel drivers, a full Windows desktop, Active Directory, and redistribution
of proprietary Microsoft content are not goals. Open an issue before you
propose work in those directions.

## License

When you contribute, you agree that your contributions have the dual
`Apache-2.0 OR MIT` license of the project.
