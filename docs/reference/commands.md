# Command reference

Most report commands accept `--format json` for machine consumption. Global
flags: `--verbose`, `--trace`, `--format human|json|csv`, and
`--env <name>` (use a named environment instead of the project's active one;
lets `ps`, `kill`, `registry`, and `service` run outside a project
directory). Commands with no machine format print a note to stderr when
`--format json|csv` is requested.

The `lsw.toml` schema and every `LSW_*` environment variable are in
[configuration.md](configuration.md). Failure codes are catalogued in
[../troubleshooting.md](../troubleshooting.md).

## Getting started

- `lsw setup` detects the project (CMake, Meson, Cargo, Zig, Make, .NET),
  creates a default environment for the `[target]` arch if none is active
  (`windows-x64` for the default `x86_64`), writes `lsw.toml` and a
  `.gitignore` if missing, and prints the next command.
- `lsw init [name] [--template console|cpp|gui|dll|service]` scaffolds a new
  project (`lsw.toml` + CMake template). The default is `console` (C).
  `cpp` is a C++ console app, `gui` uses WinMain, `dll` makes a shared
  library, and `service` is a Windows service skeleton that pairs with
  `lsw service create|start|stop`.
- `lsw doctor` diagnoses host, runtime, toolchain, and project health.
- `lsw check [--headless]` validates the project in one pass: configuration,
  environment, build, wine execution (tests or a console smoke run),
  DLL-dependency resolution, and baseline PE hardening. Nonzero exit on any
  failure.

## Build / run

- `lsw build [--system cmake|cargo|make|ninja|meson|zig|dotnet|explicit]
  [--reproducible] [--update-lock] [--aot]` builds the project; `explicit`
  forces the `[build]` command from `lsw.toml`. An unknown `--system` value
  is an error (LSW2046), never silently ignored.
- `lsw run [--host|--windows] [--sandbox strict] [--headless]
  [--dump-on-crash] [program]` starts a program; omit the program to build and
  run the project's single executable.
- `lsw exec [--host|--windows] <cmd>` runs one command in an explicit domain.
- `lsw test [--headless] [--junit <file>]` runs the tests and shows the true
  compatibility status. ctest, `cargo test` (run under Wine via a target
  runner), `meson test`, and an explicit `[test]` command are supported;
  `--junit` writes a JUnit XML report for ctest and meson runs.
- `lsw shell [--windows]` opens a shell (cmd.exe with `--windows`).
- `lsw watch [--run|--test]` rebuilds automatically when source files change;
  `--run` restarts the project executable after each build, `--test` reruns
  the suite.
- `lsw clean [--deps]` removes `build/` and `dist/` (and `deps/` with
  `--deps`).

## Binary analysis

Analysis commands accept an explicit file, or omit it to build and use the
project's artifact.

- `lsw inspect [pe]` shows format, architecture, subsystem, entry point,
  sections, embedded manifest/version-info/icon, and imports.
- `lsw audit [pe]` reports hardening (ASLR, DEP, CFG, SafeSEH, signing) as
  enabled, disabled, or not applicable to the target architecture.
- `lsw exports [dll]` lists exported symbols.
- `lsw deps tree [pe]` shows the transitive DLL tree.
- `lsw diff <a.exe> <b.exe>` compares imports, exports, sections, and size.
- `lsw size [pe] [--baseline <pe>] [--max-growth <pct>]` breaks down size by
  section bucket.
- `lsw strings [file] [--min <n>]` lists printable ASCII/UTF-16 strings.
- `lsw sbom [pe]` writes a CycloneDX SBOM.
- `lsw crash <dump.dmp>` decodes a Windows minidump (exception, module,
  address). `lsw crash --force <pe>` launches the program and forces a dump.

## Dependencies

- `lsw deps add|remove <name>` installs or removes prebuilt mingw-w64
  libraries (headers, import/static libraries, DLLs) in the project `deps/`
  sysroot and records them in `[dependencies]`; `lsw deps list` shows what is
  installed. Builds find the include and library paths automatically.
  `lsw deps tree [pe]` (under Binary analysis) is unrelated: it shows a
  binary's DLL import tree.

## Compatibility and tracing

- `lsw compat [--db] [--native] [pe]` produces a measured report of imported
  DLLs, imported API functions, and the runtime trace. `--native` adds a real
  Windows verdict from the `[verify]` host. Results are scoped: local numbers
  reflect the Wine runtime, not native Windows.
- `lsw compat-query <dll|module!func>` queries the persistent compatibility
  database.
- `lsw trace [--relay] [--filter <glob>] [pe]` shows DLL loads, registry and
  filesystem access, and any unsupported APIs observed in that Wine execution,
  plus an event timeline (`--format csv|json`).

## Debugging

- `lsw debug <pe> [--gdb [--no-start]]` starts winedbg or a gdb-remote stub
  that gdb/lldb can attach to.
- `lsw debug --native <pe> [--analyze|--interactive]` runs the binary under
  cdb on the `[verify]` Windows host for a real backtrace, `!analyze -v`
  automation, or an interactive session.
- `lsw dap` runs a Debug Adapter Protocol server on stdio for IDEs.

## Packaging and signing

- `lsw package [--target portable-directory|zip|msi|msix] [--verify]
  [--bundle-deps]` assembles a distributable. `--bundle-deps` copies each
  artifact's non-system DLL closure into the package. `--target msi --verify`
  install-tests the MSI in a scratch environment.
- `lsw sign <pe> [--publisher <subject>] [--pfx <path> --pfx-pass-env <VAR>]
  [--timestamp-url <url>]` adds an Authenticode signature (self-signed dev
  identity by default).

## Native verification

- `lsw verify [--native]` builds, then runs the artifacts on a real
  Windows host configured in `[verify]` (SSH, WinRM, or WinRM-over-TLS;
  WS-Man transports read `LSW_WINRM_PASSWORD`). Native verification is the
  default; `--native` (alias `--native-windows`) is accepted for
  explicitness. Result is `WINDOWS_VERIFIED` or `WINDOWS_UNAVAILABLE` -
  deliberately distinct from local Wine results.
- `lsw verify --reproducible [artifact]` builds twice and proves the artifacts
  are byte-identical, reporting diverging sections on mismatch.

## Environment state

- `lsw env create <name> [--arch <arch>] [--toolchain <id>] [--sdk <name>]
  [--force] [--expose-home]` creates an isolated environment (Wine prefix +
  toolchain probe); `lsw env list|clone|restore|remove` manage them.
  `lsw use <name>` selects the active one.
- `lsw registry get|set|seed|export|import|reset` (alias `reg`) manages the
  environment's isolated registry.
- `lsw ps [--all]`, `lsw kill <pid>|--all` manage runtime processes.
- `lsw service create|start|stop|query|delete` manages Windows services.
- `lsw sdk import|list|remove` manages user-supplied Windows SDK sysroots for
  MSVC-ABI builds.
- `lsw toolchain install llvm-mingw[@version]` downloads a self-contained
  llvm-mingw release into `~/.local/share/lsw/toolchains` (default: latest);
  managed toolchains are found automatically by `lsw env create`, newest
  first. `lsw toolchain list|remove` manage them.
- `lsw env provision winetricks <verbs...>` runs winetricks against the
  environment's prefix (e.g. `dxvk`, `vkd3d`, `corefonts`).
- `lsw path --to-windows <linux-path>|--to-linux <windows-path>` converts a path between views (old `--windows`/`--linux` spellings remain as aliases).

## Languages

- `lsw rust init|doctor` scaffolds and diagnoses Rust->Windows projects.
- `lsw dotnet init|doctor` scaffolds and diagnoses C#/.NET projects.

## Integration and tooling

- `lsw ide env` prints the environment description IDE plugins consume. The
  VS Code, Neovim, and JetBrains front-ends are in `editors/`.
- `lsw plugin list` discovers out-of-process `lsw-provider-*` JSON-RPC
  providers on `PATH` (reference implementation in
  `crates/lsw-provider-example`). The plugin surface is experimental:
  providers are discovered and handshaked but not yet consulted by build or
  runtime resolution.
- `lswd` is an optional background daemon managed with
  `lsw daemon start|status|stop`. It currently only answers version/ping
  probes; normal use does not need it.
- `lsw ci init github` writes a GitHub Actions workflow.
- `lsw config check` lints `lsw.toml`.
- `lsw explain LSW2004` explains an error code.
- `lsw completions bash|zsh|fish|powershell|elvish` writes shell completions;
  `lsw man [--dir <out>]` writes man pages; `lsw install [--prefix <dir>]`
  installs bash/zsh/fish completions and man pages (default prefix
  `$PREFIX`, else `~/.local`).
