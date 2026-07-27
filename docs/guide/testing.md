# Testing and debugging

## Run tests under Wine

`lsw test` builds the project, then runs the test suite through the
environment's Wine. LSW selects the test runner:

| Project | Runner |
|---------|--------|
| CMake with tests | `ctest` |
| Rust | `cargo test`, with a Wine target runner |
| Meson (configured build) | `meson test` |
| Any, with `[test] command` in `lsw.toml` | your command |

The report separates the build result, the runtime result, and the native
result. The verdict for a local pass is `LOCAL_COMPATIBILITY_VERIFIED`.
This is a Wine pass. It is not a Windows pass.

Options:

- `--headless` - run GUI tests under a virtual display (`xvfb-run`) and
  export `LSW_HEADLESS=1` to the suite.
- `--junit <file>` - write a JUnit XML report (ctest and meson runs).
- `--coverage` - instrument with clang source coverage and show an
  llvm-cov report. This needs the llvm-mingw toolchain.

`lsw check` wraps the same execution into one validation pass:
configuration, environment, build, Wine execution, DLL resolution, and PE
hardening. When the project has no tests, `lsw check` does a smoke run of
the console executable instead.

## Debug under Wine

- `lsw debug <pe>` opens the winedbg console.
- `lsw debug <pe> --gdb` starts a gdb-remote stub. Attach host gdb or lldb
  to it. Add `--no-start` to print the connection info only.
- `lsw debug --attach <pid>` attaches to a process that runs in the
  environment. `lsw ps` lists the process IDs.
- `lsw dap` is a Debug Adapter Protocol server on stdio. The VS Code
  extension uses it. `lsw ide launch-config` writes a ready
  `.vscode/launch.json`.

## Debug on native Windows

`lsw debug --native <pe>` runs the binary under cdb on the `[verify]`
Windows host and returns a real backtrace. Add `--analyze` for
`!analyze -v` triage, or `--interactive` for a live cdb session. See
[native verification](verify.md) for the host setup.

## Crash dumps

- `lsw run --dump-on-crash` captures a minidump when the program fails,
  and decodes it.
- `lsw crash <dump.dmp>` decodes a Windows minidump: the exception, the
  faulting module, and the address.
- `lsw crash --force <pe>` launches the program and forces a dump without
  a crash.

## Watch mode

`lsw watch` builds again on each source change.

- `--run` - after each good build, restart the project executable.
- `--test` - after each good build, run the test suite again.
