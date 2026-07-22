# LSW - Linux Subsystem for Windows Development

[![CI](https://github.com/johnqherman/LSW/actions/workflows/ci.yml/badge.svg)](https://github.com/johnqherman/LSW/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/lsw.svg)](https://crates.io/crates/lsw)
[![license](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)
[![msrv](https://img.shields.io/badge/rustc-1.85+-blue.svg)](rust-toolchain.toml)

LSW is a Linux-native development environment for building, running, inspecting,
debugging, packaging, and verifying Microsoft Windows applications without
leaving Linux. It is conceptually the inverse of WSL: where WSL runs a Linux
environment on Windows, LSW builds, runs, and ships Windows software on Linux.

LSW composes existing, mature technology - Wine as the execution runtime,
Clang/MinGW-w64 (GNU ABI) or clang-cl (MSVC ABI) as the cross toolchain,
CMake, Meson, Cargo, Zig, .NET, and more as build orchestrators - behind one coherent CLI
with isolated per-environment Wine prefixes, declarative `lsw.toml` project
configuration, `lsw.lock` reproducibility pinning, and deterministic
Linux<->Windows path mapping. C, C++, and Rust are first-class languages.

## Install

```
cargo install lsw && lsw install
```

`cargo install lsw` puts the `lsw` and `lswd` binaries on your `PATH`.
`lsw install` adds shell completions (bash, zsh, fish) and man pages. The shell
integration is optional. If you do not want it, use only `cargo install lsw`.

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

`lsw init --template gui|dll|console` selects the scaffold. The default is
`console`. The `gui` template uses WinMain. The `dll` template makes a shared
library.

## Command reference

- **Build / run** - `lsw build [--system cmake|cargo|make|ninja|meson|zig|dotnet]
  [--reproducible] [--update-lock]` builds the project. `lsw run
  [--host|--windows] [--sandbox strict] [--headless] <program>` starts a
  program. `lsw exec ... <cmd>` runs one command. `lsw test [--headless]` runs
  the tests and shows the true compatibility status. `lsw shell [--windows]`
  opens a shell.
- **Binary analysis** - `lsw inspect <pe>` shows the format, the architecture,
  the subsystem, the entry point, the sections, the embedded
  manifest/version-info/icon, and the imports. `lsw audit <pe>` shows the
  hardening flags. `lsw exports <dll>` lists the exports. `lsw deps tree <pe>`
  shows the transitive DLL tree. `lsw crash <dump.dmp>` decodes a Windows
  minidump and shows the exception, the module, and the address. `lsw diff
  a.exe b.exe` compares two binaries. `lsw strings <file>` lists the strings.
  `lsw sbom <pe>` writes a CycloneDX SBOM. `lsw sign <pe>` adds an Authenticode
  signature.
- **Dependencies** - `lsw deps add|remove|list <name>` installs prebuilt
  mingw-w64 libraries (headers, import/static libraries, DLLs) from the
  upstream package set into the project `deps/` sysroot. LSW records them in
  `[dependencies]`. Builds find the include paths and the library paths
  automatically.
- **Paths** - `lsw path --windows|--linux <path>` converts a path.
- **Environment state** - `lsw env create|list|clone|restore|remove`,
  `lsw registry get|set|seed|export|import|reset`, `lsw ps`,
  `lsw kill <pid>|--all`, `lsw service create|start|stop|query|delete`.
- **Compatibility** - `lsw compat [--db] [--native] <pe>` makes a measured
  report of the imported DLLs, the imported API functions, and the runtime
  trace. The `--native` option adds a real Windows verdict and a capability
  probe for each DLL and function on the `[verify]` host.
  `lsw compat-query <dll|module!func>` queries one item. `lsw trace <pe>` shows
  the DLL loads, the registry and filesystem access, and the unsupported APIs.
- **Debugging** - `lsw debug <pe> [--gdb [--no-start]]` starts winedbg or a
  gdb-remote stub. Each gdb or lldb can attach to the stub. `lsw debug --native
  <pe>` runs the binary under cdb on the `[verify]` Windows host and returns a
  real backtrace.
- **Native verification** - `lsw verify --native-windows` builds the project.
  Then it runs the artifacts on a real Windows host. Configure the host in
  `[verify]`. The transports are SSH, WinRM, and WinRM-over-TLS
  (`transport = "ssh"|"winrm"|"https"`). The WS-Man transports read
  `LSW_WINRM_PASSWORD`. The `https` transport uses port 5986. The result is
  `WINDOWS_VERIFIED` or `WINDOWS_UNAVAILABLE`. This result is not the same as
  the local Wine result.
- **Integration** - `lsw ide env` writes JSON for editor plugins. The VS Code,
  Neovim, and JetBrains front-ends are in `editors/`. `lsw dap` is a Debug
  Adapter Protocol server on stdio. `lsw plugin list` shows the out-of-process
  `lsw-provider-*` JSON-RPC providers. A reference implementation is in
  `crates/lsw-provider-example`. `lswd` is an optional daemon with `lsw daemon
  status|stop`. Normal use does not need the daemon.
- **Ergonomics / tooling** - `lsw completions bash|zsh|fish|powershell|elvish`
  writes shell completions. `lsw man [--dir <out>]` writes man pages.
  `lsw explain LSW2004` explains an error code. `lsw watch` builds again when
  the source changes. `lsw config check` examines `lsw.toml`. `lsw ci init
  github` writes a GitHub Actions workflow.

Most report commands have a `--format json` option for machine consumption. The
editor front-ends in `editors/` (a VS Code extension, a Neovim plugin, and
JetBrains External Tools) use `lsw ide env` and `lsw dap`.

## Languages and build systems

**C / C++** projects build with CMake, Meson, Ninja, or Make. LSW finds the
build system automatically. You can also set an explicit `[build]` command in
`lsw.toml`. LSW gives generated cross-toolchain files to CMake and Meson. LSW
gives the cross `CC`, `CXX`, `CFLAGS`, and `LDFLAGS` to all build systems.

LSW also finds **Zig** (`build.zig`) and **.NET** (`.csproj`/`.sln`/`.fsproj`)
projects automatically. `lsw build` runs `zig build` and `dotnet publish -r
<rid> --self-contained` for the Windows target of the environment.

LSW supports **C#** (`.csproj` found automatically). The scope is console apps
and self-contained apps:

```
lsw dotnet init hello-cs && cd hello-cs # scaffold a C# console project
lsw env create win && lsw build         # dotnet publish -r <rid> --self-contained
lsw run bin/.../hello-cs.exe            # runs under Wine
lsw dotnet doctor                       # report C#->Windows toolchain readiness
```

Builds are self-contained by default. Thus the artifact runs under Wine
without a .NET runtime in the prefix (LSW ships no runtime). C# is managed
code. It is not compiled to native code as the other languages are (cross-OS
NativeAOT is not supported). The native apphost launcher contains the runtime.
Wine gives bad support to the GUI stacks (WPF and WinForms). The supported
path is console and service apps. Use `lsw verify --native-windows` to get a
real Windows verdict.

**Rust** is a first-class language (`Cargo.toml` found automatically):

```
lsw rust init hello-rs && cd hello-rs   # scaffold a cargo project for Windows
lsw env create win && lsw build         # cargo build --target <arch>-pc-windows-gnu
lsw run target/.../hello-rs.exe         # runs under Wine
lsw rust doctor                         # report Rust->Windows toolchain readiness
```

## Target ABIs

LSW builds **GNU-ABI** binaries with MinGW-w64 by default. LSW can also build
**MSVC-ABI** binaries with clang-cl and a Windows SDK that you supply (LSW
does not redistribute SDK content):

```
lsw sdk import winsdk --from ~/splat    # import an SDK (e.g. an `xwin splat`)
lsw env create msvc --sdk winsdk        # clang-cl + lld-link, MSVC ABI
lsw build                               # produces an MSVC-ABI PE
```

`lsw sdk list` and `lsw sdk remove` manage the imported SDKs.

## Target architectures

An environment targets `x86_64` (the default), `x86`, `aarch64`, `armv7`, or
`arm64ec`. Select the architecture with `lsw env create --arch <arch>`. LSW
finds the toolchain on `$PATH` and in the directories in `$LSW_TOOLCHAIN_DIRS`
(colon-separated). Thus you can use a self-contained cross toolchain, for
example a locally extracted
[llvm-mingw](https://github.com/mstorsjo/llvm-mingw), without changes to the
system mingw-w64.

```
export LSW_TOOLCHAIN_DIRS=/path/to/llvm-mingw/bin   # for aarch64
lsw env create arm64 --arch aarch64
lsw build                                           # -> build/app.exe (ARM64 PE)
```

You can build an `aarch64` PE on an `x86_64` host. To run it locally, CPU
translation is necessary. When the target CPU family is not the host CPU
family, `lsw run` starts the program in qemu user-mode emulation with an
architecture-specific Wine. Supply the emulator (`qemu-aarch64`, `qemu-arm`).
Point `LSW_WINE_AARCH64` (or `LSW_WINE_ARM`) to the applicable Wine. If
necessary, set `QEMU_LD_PREFIX` to the sysroot of that Wine. Without these
tools, `lsw run` fails with an error that names the missing tools. Same-family
targets (an `x86` PE on `x86_64`) run directly without an emulator. You can
also verify ARM64 output on real hardware with `lsw verify --native-windows`.

A binary from `lsw build` is a genuine Windows PE executable. When you run it
under LSW, you use the local compatibility runtime (Wine). LSW does not tell
you that local runtime success is native Windows success.

## Linking: static (default) or dynamic

LSW links the C/C++ runtime statically by default. Thus the artifacts are
self-contained. They need only the DLLs of Windows 10 and later (KERNEL32 and
the UCRT api-sets). To link the shared mingw runtime, set `link = "dynamic"`
in `[toolchain]` in `lsw.toml`. LSW then puts the runtime DLLs that the binary
imports (for example `libstdc++-6.dll`, `libgcc_s_seh-1.dll`,
`libwinpthread-1.dll`, transitively) adjacent to the artifact.

## Reproducible builds

`lsw build --reproducible` makes byte-identical artifacts on each build. It
gives `-Wl,--no-insert-timestamp` to the linker. It sets the PE
`TimeDateStamp` to zero in each output binary.

## Packaging and signing

`lsw package` makes a distributable package from the build output:

```
lsw package --target portable-directory   # dist/<name>-<arch>/
lsw package --target zip                   # + .zip
lsw package --target msi                   # Windows Installer (needs wixl/msitools)
lsw package --target msix                  # signed MSIX (needs zip, osslsigncode, openssl)
```

LSW builds MSIX packages natively (manifest, block map, OPC zip). LSW signs
them with a cached self-signed identity (`~/.local/share/lsw/msix/`).
`lsw sign <pe> [--publisher <subject>]` does the same for one binary.
Self-signed artifacts install only where the certificate is trusted, or in
Windows developer mode. As with `lsw verify`, LSW does not tell you that a
Windows installation is successful.

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
default, `lsw run` runs a program with your Linux privileges. The program can
get access to the host filesystem through the `Z:` drive of Wine. The Windows
user profile does not show the host home directory, unless you made the
environment with `--expose-home`. For real isolation, use `lsw run --sandbox
strict` (bubblewrap). The sandbox has read-only system directories and a
masked home directory. Only the environment and the project are writable.
Optional CPU/memory rlimits and network modes are available. See
`SECURITY.md`.

## Environments

Environments are in `~/.local/share/lsw/environments/<name>/`.
`lsw env clone <src> <dst>` makes a fast reflink copy. `lsw env restore
<name>` builds an environment again from `lsw.lock` and makes sure that it
agrees with the pins. This makes CI reproducible.

## Notes

- `lsw shell --windows` opens `cmd` (or `powershell.exe` when the prefix has
  it) in the project directory `C:\src\<name>`. Non-interactive `lsw run`
  keeps the host working directory. Wine shows that directory under `Z:\`.
- LSW ships only its own source and the `lsw`/`lswd` binaries. LSW does not
  ship Wine, MinGW, or Microsoft SDK/CRT/redistributable content. You supply
  those, and LSW refers to them by path. The dependency licenses are
  permissive. `cargo deny` (`deny.toml`) makes sure of this. Run it before
  each release.

## License

Apache-2.0 OR MIT.
