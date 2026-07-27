# Target ABIs and architectures

## ABIs

LSW builds GNU-ABI binaries with MinGW-w64 by default. LSW can also build
MSVC-ABI binaries with clang-cl and a Windows SDK. You supply the SDK. LSW
does not redistribute SDK content.

```
lsw sdk acquire winsdk --accept-license # download through xwin
lsw env create msvc --sdk winsdk        # clang-cl + lld-link, MSVC ABI
lsw build                               # produces an MSVC-ABI PE
```

If you already have SDK content, use `lsw sdk import <name> --from <dir>`.
An `xwin splat` directory and a flat `include/lib` layout are both
accepted. `lsw sdk list` and `lsw sdk remove` manage the imported SDKs.

## Architectures

An environment targets one architecture: `x86_64` (the default), `x86`,
`aarch64`, `armv7`, or `arm64ec`. Select it with
`lsw env create --arch <arch>`, or set `[target] arch` in `lsw.toml`
before `lsw setup`.

LSW finds toolchains in this order:

1. The directories in `$LSW_TOOLCHAIN_DIRS` (colon-separated).
2. Managed installs under `~/.local/share/lsw/toolchains`, newest first.
3. `$PATH`.

A self-contained [llvm-mingw](https://github.com/mstorsjo/llvm-mingw)
covers all five architectures and does not touch the system mingw-w64.
`lsw toolchain install llvm-mingw` downloads and manages one for you:

```
lsw toolchain install llvm-mingw    # latest release
lsw env create arm64 --arch aarch64
lsw build                           # -> build/<name>.exe (ARM64 PE)
```

`lsw toolchain list` and `lsw toolchain remove` manage the installed
toolchains. `lsw doctor` shows a support matrix for the project's
architecture: the Rust target, the Zig target, the .NET RID, and the deps
repository.

## Run a cross-family binary

A same-family binary (an `x86` PE on an `x86_64` host) runs directly.

A cross-family binary (an `aarch64` PE on an `x86_64` host) runs through
qemu user-mode emulation. Prepare these items:

1. Install the emulator (`qemu-aarch64` or `qemu-arm`).
2. Set `LSW_WINE_AARCH64` or `LSW_WINE_ARM` to an architecture-specific
   Wine build.
3. If that Wine needs its sysroot, set `QEMU_LD_PREFIX`.

When an item is missing, `lsw run` fails with an error that names it. To
test ARM64 output on real hardware, use `lsw verify --native`.

## Linking: static or dynamic

LSW links the C/C++ runtime statically by default. The binaries are
self-contained. They need only the DLLs that all Windows 10 and later
installations have: KERNEL32 and the UCRT api-sets.

To link the shared mingw runtime, set `link = "dynamic"` in `[toolchain]`.
LSW then copies the runtime DLLs that the binary imports next to the
binary. Examples: `libstdc++-6.dll`, `libgcc_s_seh-1.dll`,
`libwinpthread-1.dll`, and their transitive imports.
