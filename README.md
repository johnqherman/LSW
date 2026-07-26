# LSW - Linux Subsystem for Windows Development

[![CI](https://github.com/johnqherman/LSW/actions/workflows/ci.yml/badge.svg)](https://github.com/johnqherman/LSW/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/lsw.svg)](https://crates.io/crates/lsw)
[![license](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)
[![msrv](https://img.shields.io/badge/rustc-1.85+-blue.svg)](rust-toolchain.toml)

**Build, test, and package Windows applications from Linux - with one
workflow.**

```
$ cd existing-project
$ lsw setup
Detected CMake project 'hello'
Created lsw.toml
Created environment 'windows-x64'
  toolchain llvm-mingw clang version 22.1.8
  runtime   wine 11.13

Ready:
  lsw build

$ lsw check
  + configuration    lsw.toml valid
  + environment      windows-x64 (llvm-mingw clang version 22.1.8, wine 11.13)
  + build            1 artifact(s) via CMake
  + wine execution   1 test(s) passed under wine
  + dependencies     all DLL imports resolved (1 artifact(s))
  + hardening        ASLR + DEP enabled (1 artifact(s))

all checks passed

$ lsw package
Packaged: dist/hello-x86_64
  hello.exe
Archive:  dist/hello-x86_64.zip
```

LSW configures the tools you already use - CMake, Meson, Cargo, Zig, .NET,
Clang/MinGW-w64, and Wine - it does not replace them. It puts them behind one
coherent CLI with isolated per-environment Wine prefixes, declarative
`lsw.toml` project configuration, `lsw.lock` reproducibility pinning, and
deterministic Linux<->Windows path mapping. C, C++, and Rust are first-class
languages; MSVC-ABI builds work with a user-supplied Windows SDK.

Conceptually LSW is the inverse of WSL: where WSL runs a Linux environment on
Windows, LSW builds, runs, and ships Windows software on Linux.

## Install

```
cargo install lsw && lsw install
```

`cargo install lsw` puts the `lsw` and `lswd` binaries on your `PATH`.
`lsw install` adds shell completions (bash, zsh, fish) and man pages. The shell
integration is optional. If you do not want it, use only `cargo install lsw`.

## Requirements

LSW drives tools that are already on your machine. It needs Wine, a
MinGW-w64 (or llvm-mingw) cross toolchain, and your build system. For the
default C/C++ path that means:

```
# Ubuntu / Debian
sudo apt install wine wine64 mingw-w64 cmake ninja-build

# Fedora
sudo dnf install wine mingw64-gcc mingw64-gcc-c++ cmake ninja-build

# Arch
sudo pacman -S wine mingw-w64-gcc cmake ninja

# Nix (toolchain via an llvm-mingw tarball + LSW_TOOLCHAIN_DIRS)
nix-shell -p wineWowPackages.stable cmake ninja
```

Rust projects need `rustup` with the `x86_64-pc-windows-gnu` target; .NET
projects need the `dotnet` SDK; Zig projects need `zig`. Optional features
have their own tools: `xvfb-run` (headless GUI tests), `bubblewrap`
(`--sandbox strict`), `msitools`/`wixl` (MSI), `zip`, `osslsigncode`,
`openssl` (MSIX and signing), `curl` + `tar` (`lsw deps add`), and `qemu-user`
(cross-family emulation). Nothing is downloaded behind your back.

`lsw doctor` checks all of this and names anything missing. If your Wine is
not on `PATH` (WineHQ `/opt` builds, Proton, Nix profiles), point `LSW_WINE`
at the binary.

## Quickstart

In an existing project (CMake, Meson, Cargo, Zig, Make, or .NET):

```
lsw setup                     # detect project, create env, write lsw.toml
lsw build                     # cross-compile to build/<name>.exe (real PE)
lsw run                       # build + run the executable through Wine
lsw check                     # build, wine execution, deps, hardening
lsw package                   # assemble dist/<name>-<arch>[.zip]
```

Starting from nothing:

```
lsw init hello && cd hello    # scaffold lsw.toml + CMake hello project
lsw setup                     # create + wire the default environment
lsw check                     # validate the whole pipeline
```

`lsw init --template gui|dll|console` selects the scaffold. The default is
`console`. The `gui` template uses WinMain. The `dll` template makes a shared
library.

Useful next commands: `lsw test` (test suite under Wine with honest
compatibility status), `lsw inspect` / `lsw audit` (PE analysis of the built
artifact), `lsw doctor` (host diagnosis), `lsw watch` (rebuild on change),
`lsw verify --native` (run artifacts on a real Windows host).

Advanced workflows (multiple environments, other architectures, MSVC ABI) use
`lsw env create <name> [--arch <arch>] [--sdk <name>]` and `lsw use <name>`
directly; `lsw setup` is a convenience over the same machinery.

## Commands

```
Getting started:  setup  init  doctor  check
Build / run:      build  run  exec  test  shell  watch
Analysis:         inspect  audit  exports  deps  diff  size  strings  sbom  crash
Compatibility:    compat  compat-query  trace
Debugging:        debug  dap
Ship:             package  sign  verify  ci
Environments:     env  use  registry  ps  kill  service  sdk  path
Languages:        rust  dotnet
Tooling:          ide  plugin  daemon  config  explain  completions  man  install
```

The full command reference with flags and semantics is in
[docs/reference/commands.md](docs/reference/commands.md). Most report commands
have a `--format json` option for machine consumption. If a command fails with
an `LSW####` code, `lsw explain <code>` describes it; the same catalogue is in
[docs/troubleshooting.md](docs/troubleshooting.md).

## Editor integration

- **VS Code**: `code --install-extension lsw.lsw` - build/run/test commands,
  IntelliSense configuration from `lsw ide env`, and Wine debugging through
  `lsw dap`.
- **Neovim**: point your plugin manager at `editors/nvim` and call
  `require("lsw").setup()`.
- **JetBrains**: External Tools definitions in `editors/jetbrains`.

Details in [editors/README.md](editors/README.md). Any other editor can use
`lsw ide env` (compiler, flags, include paths as JSON) and the `lsw dap`
debug adapter directly.

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
lsw setup && lsw build                  # dotnet publish -r <rid> --self-contained
lsw run                                 # runs under Wine
lsw dotnet doctor                       # report C#->Windows toolchain readiness
```

Builds are self-contained by default. Thus the artifact runs under Wine
without a .NET runtime in the prefix (LSW ships no runtime). Wine gives bad
support to the GUI stacks (WPF and WinForms). The supported path is console
and service apps. Use `lsw verify --native` to get a real Windows
verdict.

`lsw build --aot` (or `aot = true` in `[toolchain]`) compiles C# with
NativeAOT. The output is a native PE, not a managed app. The file is smaller.
The start is faster. The artifact contains no CLR. The full `inspect`,
`audit`, and `compat` tooling applies. The .NET SDK does not permit cross-OS
NativeAOT. LSW makes it possible from Linux without a Microsoft SDK: LSW
writes the MSVC CRT glue (chkstk, security cookie, TLS directory, CRT
startup), compiles the glue with the MSVC-ABI codegen of clang, maps the
mingw-w64 import libraries to their MSVC names, and links with `lld-link`. The
current scope is x86_64 console apps. The host needs `clang` and `lld-link`.
`lsw dotnet doctor` shows the NativeAOT row.

**Rust** is a first-class language (`Cargo.toml` found automatically):

```
lsw rust init hello-rs && cd hello-rs   # scaffold a cargo project for Windows
lsw setup && lsw build                  # cargo build --target <arch>-pc-windows-gnu
lsw run                                 # runs under Wine
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
also verify ARM64 output on real hardware with `lsw verify --native`.

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

`lsw package --target msi --verify` does an install test of the MSI before it
reports success. It clones the active environment to a scratch prefix. It runs
`msiexec /i` quietly. It makes sure that each packaged file is under Program
Files. It uninstalls with `msiexec /x`. It makes sure that no files remain. It
removes the scratch environment. Failures show as `LSW2040` with the msiexec
output.

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
aot  = false         # C# NativeAOT (see Languages)

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

[build]              # explicit build command (skips auto-detection)
command = ["make", "-f", "windows.mk"]

[test]               # explicit test command (run under the LSW environment)
command = ["ctest", "--test-dir", "build"]

[dependencies]       # prebuilt Windows libraries via `lsw deps add`
zlib = "1.3.1-1"
```

A generated `lsw.toml` contains only `[project]`; every other section is
optional and defaults are omitted. The full schema, including the
`[environment]`, `[runtime]`, and `[filesystem]` sections and every `LSW_*`
environment variable, is in
[docs/reference/configuration.md](docs/reference/configuration.md). Commit
both `lsw.toml` and `lsw.lock` to version control.

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
