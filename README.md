<img src="assets/logo.svg" alt="LSW" width="96" align="right"/>

# LSW - Linux Subsystem for Windows Development

[![CI](https://github.com/johnqherman/LSW/actions/workflows/ci.yml/badge.svg)](https://github.com/johnqherman/LSW/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/lsw.svg)](https://crates.io/crates/lsw)
[![license](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)
[![msrv](https://img.shields.io/badge/rustc-1.85+-blue.svg)](Cargo.toml)

**Build, run, debug, and ship Windows software without leaving Linux.**

```console
$ lsw setup
Detected CMake project 'hello'
Created environment 'windows-x64' (llvm-mingw 22.1.8, wine 11.13)

$ lsw run
Hello from LSW

$ lsw check
  + build            1 artifact(s) via CMake
  + wine execution   1 test(s) passed under wine
  + dependencies     all DLL imports resolved
  + hardening        ASLR + DEP enabled
all checks passed
```

`lsw build` produces a genuine Windows PE from your existing CMake, Meson,
Cargo, Zig, Make, or .NET project. `lsw run` executes it through Wine.
`lsw test`, `lsw debug`, and `lsw package` cover the rest of the loop.
Running under LSW means running under Wine, and LSW never reports a Wine
pass as a Windows pass - that is what `lsw verify --native` is for: it runs
your artifacts on a real Windows host and says so.

LSW is one CLI over tools you already have - MinGW-w64 or Clang for the
cross-compile, Wine for execution - plus the glue those tools never grew:
isolated per-environment Wine prefixes, a declarative `lsw.toml`, `lsw.lock`
pinning for reproducible builds, and deterministic path mapping between the
Linux and Windows views of your project. Where WSL puts a Linux userland on
Windows, LSW points the other way: the Windows target, developed entirely
from Linux.

## Install

```
cargo install lsw && lsw install
```

`cargo install lsw` puts the `lsw` and `lswd` binaries on your `PATH`.
`lsw install` adds shell completions (bash, zsh, fish) and man pages. The
shell integration is optional; skip it with plain `cargo install lsw`.

## Requirements

LSW drives tools that are already on your machine: Wine (9+ recommended,
`lsw doctor` warns on older), a MinGW-w64 or llvm-mingw cross toolchain, and
your build system. For the default C/C++ path that means:

```
# Ubuntu / Debian
sudo apt install wine wine64 mingw-w64 cmake ninja-build

# Fedora
sudo dnf install wine mingw64-gcc mingw64-gcc-c++ cmake ninja-build

# Arch
sudo pacman -S wine mingw-w64-gcc cmake ninja

# Nix (toolchain via lsw toolchain install or LSW_TOOLCHAIN_DIRS)
nix-shell -p wineWowPackages.stable cmake ninja
```

Rust projects need `rustup` with the `x86_64-pc-windows-gnu` target; .NET
projects need the `dotnet` SDK; Zig projects need `zig`. Optional features
have their own tools: `xvfb-run` (headless GUI tests), `bubblewrap`
(`--sandbox strict`), `msitools`/`wixl` (MSI), `makensis` (NSIS), `zip`,
`osslsigncode`, `openssl` (MSIX and signing), `curl` + `tar`
(`lsw deps add`), and `qemu-user` (cross-family emulation).

LSW ships only its own source and binaries - no Wine, no MinGW, no Microsoft
SDK or redistributable content. You supply those; nothing is downloaded
behind your back, and every download LSW can make on request is documented
in [SECURITY.md](SECURITY.md). `lsw doctor` checks the whole setup and names
anything missing. If your Wine is not on `PATH` (WineHQ `/opt` builds,
Proton, Nix profiles), point `LSW_WINE` at the binary.

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

`lsw init --template console|cpp|gui|dll|service` picks the scaffold.
Useful next commands: `lsw test` (test suite under Wine), `lsw inspect` /
`lsw audit` (PE analysis), `lsw doctor` (host diagnosis), `lsw watch`
(rebuild on change), `lsw verify --native` (real Windows verdict).
Multiple environments, other architectures, and the MSVC ABI use
`lsw env create` and `lsw use` directly; `lsw setup` is a convenience over
the same machinery.

## Commands

```
Getting started:  setup  init  doctor  check
Build / run:      build  run  exec  test  shell  watch  clean
Analysis:         inspect  audit  exports  deps  diff  size  strings  sbom  crash
Compatibility:    compat  compat-query  trace
Debugging:        debug  dap
Ship:             package  sign  verify  ci
Environments:     env  use  registry  ps  kill  service  sdk  toolchain  path
Languages:        rust  dotnet
Tooling:          ide  plugin  daemon  config  explain  completions  man  install
```

The full command reference with flags and semantics is in
[docs/reference/commands.md](docs/reference/commands.md). Most report
commands take `--format json`. If a command fails with an `LSW####` code,
`lsw explain <code>` describes it; the same catalogue is in
[docs/troubleshooting.md](docs/troubleshooting.md).

## Languages and build systems

**C / C++** projects build with CMake, Meson, Ninja, or Make - detected
automatically, or forced with an explicit `[build]` command in `lsw.toml`.
LSW generates cross-toolchain files for CMake and Meson, and exports the
cross `CC`, `CXX`, `CFLAGS`, and `LDFLAGS` to every build system.

**Rust** is first-class (`Cargo.toml` detected automatically):

```
lsw rust init hello-rs && cd hello-rs   # scaffold a cargo project for Windows
lsw setup && lsw build                  # cargo build --target <arch>-pc-windows-gnu
lsw test                                # cargo test, run under Wine
lsw rust doctor                         # report Rust->Windows readiness
```

**C# / .NET** (`.csproj`/`.sln`/`.fsproj` detected automatically) builds
with `dotnet publish -r <rid> --self-contained`, so the artifact runs under
Wine with no .NET runtime in the prefix (LSW ships none). WPF and WinForms
run poorly under Wine; the supported path is console and service apps, with
`lsw verify --native` for the real Windows verdict. `lsw build --aot`
compiles C# with NativeAOT to a native PE: smaller, faster to start, no
CLR, and the full `inspect`/`audit`/`compat` tooling applies. The .NET SDK
does not permit cross-OS NativeAOT; LSW supplies the MSVC CRT glue itself
and links with `lld-link`, so it works from Linux without a Microsoft SDK
(needs `clang` and `lld-link`; x86_64 console apps only - `lsw dotnet
doctor` shows the row).

**Zig** (`build.zig`) builds via `zig build` for the environment's Windows
target.

## Target ABIs

LSW builds **GNU-ABI** binaries with MinGW-w64 by default. It can also build
**MSVC-ABI** binaries with clang-cl and a Windows SDK that you supply (LSW
does not redistribute SDK content):

```
lsw sdk acquire winsdk --accept-license # download via xwin, or: lsw sdk import
lsw env create msvc --sdk winsdk        # clang-cl + lld-link, MSVC ABI
lsw build                               # produces an MSVC-ABI PE
```

## Target architectures

An environment targets `x86_64` (the default), `x86`, `aarch64`, `armv7`,
or `arm64ec`; pick with `lsw env create --arch <arch>`. Toolchains are found
in `$LSW_TOOLCHAIN_DIRS`, in managed installs under
`~/.local/share/lsw/toolchains`, then on `$PATH` - so a self-contained
[llvm-mingw](https://github.com/mstorsjo/llvm-mingw) works without touching
the system mingw-w64, and `lsw toolchain install llvm-mingw` manages one
for you:

```
lsw toolchain install llvm-mingw    # latest release, all five architectures
lsw env create arm64 --arch aarch64
lsw build                           # -> build/<name>.exe (ARM64 PE)
```

Cross-family targets (an `aarch64` PE on an `x86_64` host) run through qemu
user-mode emulation: supply the emulator (`qemu-aarch64`, `qemu-arm`), point
`LSW_WINE_AARCH64` or `LSW_WINE_ARM` at an architecture-specific Wine, and
set `QEMU_LD_PREFIX` if that Wine needs its sysroot. Missing pieces produce
an error that names them. Same-family targets (an `x86` PE on `x86_64`) run
directly, and `lsw verify --native` covers real ARM64 hardware.

## Linking: static (default) or dynamic

LSW links the C/C++ runtime statically by default, so artifacts are
self-contained and need only DLLs every Windows 10+ install has (KERNEL32,
the UCRT api-sets). Set `link = "dynamic"` in `[toolchain]` to link the
shared mingw runtime instead; LSW then places the runtime DLLs the binary
imports (`libstdc++-6.dll`, `libgcc_s_seh-1.dll`, `libwinpthread-1.dll`,
transitively) next to the artifact.

## Reproducible builds

`lsw build --reproducible` makes byte-identical artifacts: it passes
`-Wl,--no-insert-timestamp` to the linker and zeroes the PE `TimeDateStamp`
in every output. `lsw verify --reproducible` proves it by building twice.

## Packaging and signing

`lsw package` turns the build output into a distributable:

```
lsw package --target portable-directory   # dist/<name>-<arch>/
lsw package --target zip                  # + .zip
lsw package --target msi                  # Windows Installer (needs wixl/msitools)
lsw package --target msix                 # signed MSIX (needs zip, osslsigncode, openssl)
lsw package --target nsis                 # NSIS setup.exe (needs makensis)
lsw package --target winget               # MSI + winget manifests
```

A `[package]` section in `lsw.toml` (version, publisher, description, icon,
shortcuts) feeds the installer metadata and is embedded into the built PE
as an icon, VERSIONINFO resource, and application manifest.
`lsw package --target msi --verify` installs the MSI quietly into a scratch
clone of the environment, checks every file landed under Program Files,
uninstalls, and verifies nothing was left behind (failures show as
`LSW2040`).

MSIX packages are built natively (manifest, block map, OPC zip) and signed
with a cached self-signed identity; `lsw sign <pe>` does the same for one
binary, or signs with a real PFX certificate. Self-signed artifacts install
only where the certificate is trusted, or in Windows developer mode.

## Project configuration (`lsw.toml`)

```toml
[project]
name = "hello"

[target]
arch = "x86_64"      # x86_64 | x86 | aarch64 | armv7 | arm64ec
api  = "win10"       # -> _WIN32_WINNT/WINVER/NTDDI defines

[toolchain]
link = "dynamic"     # static (default) | dynamic

[build]              # explicit build command (skips auto-detection)
command = ["make", "-f", "windows.mk"]
```

A generated `lsw.toml` contains only `[project]`; everything else is
optional. The full schema - `[package]`, `[env]`, `[registry]`, `[sandbox]`,
`[verify]`, `[dependencies]`, and every `LSW_*` environment variable - is in
[docs/reference/configuration.md](docs/reference/configuration.md). Commit
both `lsw.toml` and `lsw.lock` to version control.

## Sandboxing and security

The Wine prefix is a **compatibility boundary, not a security boundary**. By
default `lsw run` executes a program with your Linux privileges, and
programs can reach the host filesystem through Wine's `Z:` drive. The
Windows user profile hides your host home directory unless the environment
was created with `--expose-home`. For real isolation, `lsw run --sandbox
strict` runs the program under bubblewrap: read-only system directories,
masked home, only the environment and project writable, with optional
CPU/memory rlimits and network modes. Details in
[SECURITY.md](SECURITY.md).

## Environments

Environments live in `~/.local/share/lsw/environments/<name>/`.
`lsw env clone` makes a fast reflink copy, `lsw env restore <name>` rebuilds
an environment from `lsw.lock` and verifies it matches the pins - the basis
of reproducible CI - and `lsw env export` / `lsw env import-archive` move
whole environments between machines (CI cache handoff).

## Editor integration

- **VS Code**: `code --install-extension lsw.lsw` - build/test tasks with
  clickable errors, IntelliSense configuration from `lsw ide env`, and Wine
  debugging through `lsw dap`.
- **Neovim**: point your plugin manager at `editors/nvim` and call
  `require("lsw").setup()`.
- **JetBrains**: External Tools definitions in `editors/jetbrains`.

Details in [editors/README.md](editors/README.md). Any other editor can use
`lsw ide env` (compiler, flags, include paths as JSON) and the `lsw dap`
debug adapter directly.

## License

Apache-2.0 OR MIT.
