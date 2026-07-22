# LSW - Linux Subsystem for Windows Development

LSW is a Linux-native development environment for building, running, and
inspecting Microsoft Windows applications without leaving Linux. It is
conceptually the inverse of WSL: where WSL hosts a Linux environment on
Windows, LSW makes Windows a first-class *target* from a Linux host.

LSW composes existing, mature technology - Wine as the execution runtime,
Clang/MinGW-w64 (GNU ABI) or clang-cl (MSVC ABI) as the cross toolchain,
CMake/Meson/Ninja/Make/Cargo as build orchestrators - behind one coherent CLI
with isolated per-environment Wine prefixes, declarative `lsw.toml` project
configuration, `lsw.lock` reproducibility pinning, and deterministic
Linux<->Windows path mapping. C, C++, and Rust are first-class languages.

## Install

```
cargo install lsw && lsw install
```

`cargo install lsw` puts the `lsw` and `lswd` binaries on your `PATH`;
`lsw install` adds shell completions (bash/zsh/fish) and man pages. Just
`cargo install lsw` alone is enough if you skip the shell integration.

## Quickstart

```
lsw init hello && cd hello    # scaffold lsw.toml + CMake hello project
lsw env create win11-x64      # isolated Wine prefix + toolchain probe
lsw use win11-x64             # select the active environment
lsw build                     # cross-compile to build/hello.exe (real PE)
lsw run build/hello.exe       # execute locally through the Wine runtime
lsw test                      # run tests under the runtime (honest compat status)
lsw inspect build/hello.exe   # PE format, arch, subsystem, sections, imports
lsw audit build/hello.exe     # security hardening (ASLR/DEP/CFG/signed)
lsw package                   # assemble dist/<name>-<arch>[.zip]
lsw doctor                    # diagnose host / runtime / toolchain / project
lsw watch                     # rebuild automatically on source changes
```

`lsw init --template gui|dll|console` picks the scaffold (console is the
default; `gui` uses WinMain, `dll` builds a shared library).

## Command reference

- **Build / run** - `lsw build [--system cmake|cargo|make|ninja|meson]
  [--reproducible] [--update-lock]`, `lsw run [--host|--windows]
  [--sandbox strict] [--headless] <program>`, `lsw exec ... <cmd>`,
  `lsw test [--headless]`, `lsw shell [--windows]`.
- **Binary analysis** - `lsw inspect <pe>` (format, arch, subsystem, entry
  point, sections, embedded manifest/version-info/icon, imports), `lsw audit
  <pe>` (hardening flags), `lsw exports <dll>`, `lsw deps tree <pe>` (transitive
  DLL tree), `lsw crash <dump.dmp>` (decode a Windows minidump: exception,
  faulting module, address), `lsw diff a.exe b.exe`, `lsw strings <file>`,
  `lsw sbom <pe>` (CycloneDX), `lsw sign <pe>` (Authenticode).
- **Dependencies** - `lsw deps add|remove|list <name>` installs prebuilt
  mingw-w64 libraries (headers, import/static libs, DLLs) from the upstream
  package set into the project's `deps/` sysroot, recorded in `[dependencies]`;
  builds pick up the include and library paths automatically.
- **Paths** - `lsw path --windows|--linux <path>`.
- **Environment state** - `lsw env create|list|clone|restore|remove`,
  `lsw registry get|set|seed|export|import|reset`, `lsw ps`,
  `lsw kill <pid>|--all`, `lsw service create|start|stop|query|delete`.
- **Compatibility** - `lsw compat [--db] [--native] <pe>` (measured report:
  imported DLLs, imported API functions, runtime trace; `--native` adds a real
  Windows verdict plus a per-DLL/function capability probe on the `[verify]`
  host), `lsw compat-query <dll|module!func>`, `lsw trace <pe>` (DLL loads,
  registry/filesystem access, unsupported APIs).
- **Debugging** - `lsw debug <pe> [--gdb [--no-start]]` (winedbg, or a
  gdb-remote stub any gdb/lldb can attach to); `lsw debug --native <pe>` runs the
  binary under cdb on the `[verify]` Windows host and returns a real backtrace.
- **Native verification** - `lsw verify --native-windows` builds, then runs the
  artifacts on a real Windows host (configured in `[verify]`) over SSH or WinRM
  (`transport = "ssh"|"winrm"`; WinRM reads `LSW_WINRM_PASSWORD`), yielding an
  honest `WINDOWS_VERIFIED` / `WINDOWS_UNAVAILABLE` status distinct from the
  local Wine result.
- **Integration** - `lsw ide env` (JSON for editor plugins), `lsw dap` (a Debug
  Adapter Protocol server over stdio), `lsw plugin list` (out-of-process
  `lsw-provider-*` JSON-RPC providers), `lswd` + `lsw daemon status|stop` (an
  optional daemon; not required for normal use).
- **Ergonomics / tooling** - `lsw completions bash|zsh|fish|powershell|elvish`,
  `lsw man [--dir <out>]`, `lsw explain LSW2004` (explain an error code),
  `lsw watch`, `lsw config check` (lint `lsw.toml`), `lsw ci init github`
  (generate a GitHub Actions workflow).

Most report-style commands accept `--format json` for machine consumption.
Editor packages (VS Code extension, Neovim plugin) live under `editors/` and
shell out to `lsw ide env` and `lsw dap`.

## Languages and build systems

**C / C++** build through CMake, Meson, Ninja, or Make (auto-detected), or an
explicit `[build]` command in `lsw.toml`. CMake and Meson receive generated
cross-toolchain files; all systems get the cross `CC`/`CXX`/`CFLAGS`/`LDFLAGS`.

**Rust** is first-class (`Cargo.toml` auto-detected):

```
lsw rust init hello-rs && cd hello-rs   # scaffold a cargo project for Windows
lsw env create win && lsw build         # cargo build --target <arch>-pc-windows-gnu
lsw run target/.../hello-rs.exe         # runs under Wine
lsw rust doctor                         # report Rust->Windows toolchain readiness
```

## Target ABIs

By default LSW builds **GNU-ABI** binaries with MinGW-w64. It can also build
**MSVC-ABI** binaries with clang-cl against a Windows SDK you supply (LSW never
redistributes SDK content):

```
lsw sdk import winsdk --from ~/splat    # import an SDK (e.g. an `xwin splat`)
lsw env create msvc --sdk winsdk        # clang-cl + lld-link, MSVC ABI
lsw build                               # produces an MSVC-ABI PE
```

`lsw sdk list` / `lsw sdk remove` manage imported SDKs.

## Target architectures

Environments target `x86_64` (default), `x86`, `aarch64`, `armv7`, or
`arm64ec` via `lsw env create --arch <arch>`. The toolchain is discovered from
`$PATH` and, in addition, from any directories listed in `$LSW_TOOLCHAIN_DIRS`
(colon-separated) - so a self-contained cross toolchain such as a locally
extracted [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) can be used
without touching the system mingw-w64 install.

```
export LSW_TOOLCHAIN_DIRS=/path/to/llvm-mingw/bin   # for aarch64
lsw env create arm64 --arch aarch64
lsw build                                           # -> build/app.exe (ARM64 PE)
```

Building an `aarch64` PE works on an `x86_64` host, but *running* it locally
does not: Wine cannot execute a foreign-architecture PE without CPU
translation, which lives outside LSW's core. `lsw run` fails honestly ("Bad
format") rather than pretending; verify ARM64 output with `file` /
`lsw inspect`, or on real hardware via `lsw verify --native-windows`.

A binary produced by `lsw build` is a genuine Windows PE executable; running
it under LSW exercises the local compatibility runtime (Wine). LSW never
equates local runtime success with native Windows success.

## Linking: static (default) or dynamic

By default LSW links the C/C++ runtime statically, so artifacts are
self-contained (they need only DLLs shipped with Windows 10+ - KERNEL32 + the
UCRT api-sets). Set `link = "dynamic"` under `[toolchain]` in `lsw.toml` to link
the shared mingw runtime instead; LSW then automatically deploys the runtime
DLLs the binary imports (e.g. `libstdc++-6.dll`, `libgcc_s_seh-1.dll`,
`libwinpthread-1.dll`, transitively) next to the artifact.

## Reproducible builds

`lsw build --reproducible` produces byte-identical artifacts across rebuilds:
it passes `-Wl,--no-insert-timestamp` to the linker and zeroes the PE
`TimeDateStamp` in every produced binary.

## Packaging and signing

`lsw package` assembles a distributable from the build output:

```
lsw package --target portable-directory   # dist/<name>-<arch>/
lsw package --target zip                   # + .zip
lsw package --target msi                   # Windows Installer (needs wixl/msitools)
lsw package --target msix                  # signed MSIX (needs zip + osslsigncode)
```

MSIX packages are built natively - LSW generates the `AppxManifest.xml`,
`AppxBlockMap.xml` (SHA-256 block hashes) and content types, zips the OPC
package, and signs it with a cached self-signed identity
(`~/.local/share/lsw/msix/`). `lsw sign <pe> [--publisher <subject>]`
Authenticode-signs a standalone binary with the same identity. Self-signed
artifacts install only where the certificate is trusted (or in Windows
developer mode); as with `lsw verify`, actual Windows installation is not
asserted.

## Project configuration (`lsw.toml`)

```toml
[project]
name = "hello"

[target]
arch = "x86_64"      # x86_64 | x86 | aarch64 | armv7 | arm64ec
api  = "win10"       # -> _WIN32_WINNT/WINVER/NTDDI defines (win7/win8/win10/win11/...)

[toolchain]
link = "dynamic"     # static (default) | dynamic

[env.vars]           # extra Windows env vars for run/exec
RUST_LOG = "debug"
[env.secret]         # inject a host env var by name (kept out of the manifest)
API_TOKEN = "HOST_API_TOKEN"

[[registry.seed]]    # applied by `lsw registry seed`
key   = "HKCU\\Software\\Hello"
name  = "FirstRun"
value = "1"
type  = "dword"      # string (default) | dword | expand

[sandbox]            # for `lsw run --sandbox strict`
network     = "host" # host | isolated (NAT via pasta/slirp4netns) | none
cpu_seconds = 30
memory_mb   = 2048

[verify]             # native Windows verification host
transport     = "ssh"
host          = "user@win-host"
identity_file = "~/.ssh/lsw_verify"
```

## Sandboxing and security

The Wine prefix is a **compatibility boundary, not a security boundary**. By
default `lsw run` executes with your Linux privileges and can reach the host
filesystem via Wine's `Z:` drive; the host home directory is hidden from the
Windows user profile unless the environment was created with `--expose-home`.
Real isolation comes from `lsw run --sandbox strict` (bubblewrap: read-only
system dirs, masked home, only the environment and project writable, with
optional CPU/memory rlimits and network modes). See `SECURITY.md` and
`docs/sandboxing.md`.

## Environments

Environments live under `~/.local/share/lsw/environments/<name>/`.
`lsw env clone <src> <dst>` makes a cheap reflink copy; `lsw env restore <name>`
rebuilds an environment from `lsw.lock` and verifies it matches the pins, which
is what makes CI reproducible.

## Notes

- `lsw shell --windows` opens `cmd` (or `powershell.exe` when the prefix has it)
  at the project's `C:\src\<name>` directory. Non-interactive `lsw run` still
  inherits the host working directory, which Wine surfaces under `Z:\`.
- Further design docs live under `docs/` (environments, toolchains, sandboxing,
  compatibility). Dependency/redistribution policy is in `docs/AUDIT.md`
  (`cargo deny check` via `deny.toml`).

## License

Apache-2.0 OR MIT.
