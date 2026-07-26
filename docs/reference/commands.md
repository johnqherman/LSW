# Command reference

Most report commands accept `--format json` for machine consumption. Global
flags: `--verbose`, `--trace`, `--format human|json|csv`.

The `lsw.toml` schema and every `LSW_*` environment variable are in
[configuration.md](configuration.md). Failure codes are catalogued in
[../troubleshooting.md](../troubleshooting.md).

## Getting started

- `lsw setup` detects the project (CMake, Meson, Cargo, Zig, Make, .NET),
  creates a default environment for the `[target]` arch if none is active
  (`windows-x64` for the default `x86_64`), writes `lsw.toml` and a
  `.gitignore` if missing, and prints the next command.
- `lsw init [name] [--template console|gui|dll]` scaffolds a new project
  (`lsw.toml` + CMake template). The default template is `console`. The `gui`
  template uses WinMain. The `dll` template makes a shared library.
- `lsw doctor` diagnoses host, runtime, toolchain, and project health.
- `lsw check [--headless]` validates the project in one pass: configuration,
  environment, build, wine execution (tests or a console smoke run),
  DLL-dependency resolution, and baseline PE hardening. Nonzero exit on any
  failure.

## Build / run

- `lsw build [--system cmake|cargo|make|ninja|meson|zig|dotnet]
  [--reproducible] [--update-lock] [--aot]` builds the project.
- `lsw run [--host|--windows] [--sandbox strict] [--headless]
  [--dump-on-crash] [program]` starts a program; omit the program to build and
  run the project's single executable.
- `lsw exec [--host|--windows] <cmd>` runs one command in an explicit domain.
- `lsw test [--headless]` runs the tests and shows the true compatibility
  status.
- `lsw shell [--windows]` opens a shell (cmd.exe with `--windows`).
- `lsw watch` rebuilds automatically when source files change.

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

- `lsw verify --native-windows` builds, then runs the artifacts on a real
  Windows host configured in `[verify]` (SSH, WinRM, or WinRM-over-TLS;
  WS-Man transports read `LSW_WINRM_PASSWORD`). Result is `WINDOWS_VERIFIED`
  or `WINDOWS_UNAVAILABLE` - deliberately distinct from local Wine results.
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
- `lsw path --windows|--linux <path>` converts a path between views.

## Languages

- `lsw rust init|doctor` scaffolds and diagnoses Rust->Windows projects.
- `lsw dotnet init|doctor` scaffolds and diagnoses C#/.NET projects.

## Integration and tooling

- `lsw ide env` prints the environment description IDE plugins consume. The
  VS Code, Neovim, and JetBrains front-ends are in `editors/`.
- `lsw plugin list` shows out-of-process `lsw-provider-*` JSON-RPC providers
  (reference implementation in `crates/lsw-provider-example`).
- `lswd` is an optional daemon with `lsw daemon status|stop`; normal use does
  not need it.
- `lsw ci init github` writes a GitHub Actions workflow.
- `lsw config check` lints `lsw.toml`.
- `lsw explain LSW2004` explains an error code.
- `lsw completions bash|zsh|fish|powershell|elvish` writes shell completions;
  `lsw man [--dir <out>]` writes man pages; `lsw install [--prefix <dir>]`
  installs bash/zsh/fish completions and man pages (default prefix
  `$PREFIX`, else `~/.local`).
