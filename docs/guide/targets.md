# Target ABIs and architectures

## ABIs

LSW builds **GNU-ABI** binaries with MinGW-w64 by default. It can also build
**MSVC-ABI** binaries with clang-cl and a Windows SDK that you supply (LSW
does not redistribute SDK content):

```
lsw sdk acquire winsdk --accept-license # download via xwin, or: lsw sdk import
lsw env create msvc --sdk winsdk        # clang-cl + lld-link, MSVC ABI
lsw build                               # produces an MSVC-ABI PE
```

## Architectures

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

`lsw doctor` prints a per-architecture support matrix (Rust target, Zig
target, .NET RID, deps repository) for the active project's arch.

## Running cross-family targets

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
