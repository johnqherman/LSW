# Testing and debugging

## Tests under Wine

`lsw test` builds, then runs your suite through the environment's Wine:
ctest (CMake), `cargo test` (via a Wine target runner), `meson test`, or an
explicit `[test]` command from `lsw.toml`. The report separates build,
runtime, and native outcomes, and the compatibility verdict is deliberately
labeled `LOCAL_COMPATIBILITY_VERIFIED` - a Wine pass, not a Windows pass.

- `lsw test --headless` runs GUI tests under a virtual display
  (`xvfb-run`) and exports `LSW_HEADLESS=1` to the suite.
- `lsw test --junit report.xml` writes a JUnit report (ctest and meson).
- `lsw test --coverage` instruments with clang source coverage and prints
  an llvm-cov region/line report (llvm-mingw toolchain only).

`lsw check` wraps the same execution into a one-shot pipeline validation:
configuration, environment, build, wine execution, DLL resolution, and PE
hardening.

## Debugging

- `lsw debug <pe>` opens winedbg; `--gdb` starts a gdb-remote stub for
  host gdb/lldb, `--attach <pid>` attaches to a running process
  (`lsw ps` lists pids).
- `lsw dap` is a Debug Adapter Protocol server over stdio - the VS Code
  extension uses it, and `lsw ide launch-config` writes a ready
  `.vscode/launch.json`.
- `lsw debug --native <pe>` runs the binary under cdb on the `[verify]`
  Windows host for a real backtrace or `!analyze -v` triage.
- `lsw run --dump-on-crash` captures a minidump on failure; `lsw crash`
  decodes any Windows minidump (exception, faulting module, address).

## Watch mode

`lsw watch` rebuilds on source change; `--run` restarts the project
executable after each successful build, `--test` reruns the suite.
