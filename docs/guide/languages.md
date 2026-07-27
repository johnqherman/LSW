# Languages and build systems

LSW examines the project directory and selects the build system. The
detection order is: CMake, Meson, Zig, Cargo, .NET, Ninja, Make. To force a
different system, use `lsw build --system <name>`. To use your own command,
set `[build] command` in `lsw.toml`.

## C and C++

C and C++ projects build with CMake, Meson, Ninja, or Make.

- LSW generates a cross-toolchain file for CMake and a cross file for
  Meson.
- LSW exports the cross `CC`, `CXX`, `CFLAGS`, and `LDFLAGS` to all build
  systems.
- Set `[toolchain] ccache = true` to put ccache in front of CMake
  compilation.

Build commands also receive these variables: `LSW_ENV` (the environment
name), `LSW_PROJECT` (the project root), and `LSW_TARGET_FLAGS` (the cross
C flags).

## Rust

LSW detects `Cargo.toml` automatically. Rust is a first-class language.

```
lsw rust init hello-rs && cd hello-rs
lsw setup && lsw build      # cargo build --target <arch>-pc-windows-gnu
lsw test                    # cargo test, run under Wine
lsw rust doctor             # show the Rust->Windows readiness
```

`lsw test` sets a Cargo target runner, so the test binaries run under
Wine.

## C# and .NET

LSW detects `.csproj`, `.sln`, and `.fsproj` files automatically.

```
lsw dotnet init hello-cs && cd hello-cs
lsw setup && lsw build      # dotnet publish -r <rid> --self-contained
lsw run                     # runs under Wine
lsw dotnet doctor           # show the C#->Windows readiness
```

Builds are self-contained. The binary runs under Wine without a .NET
runtime in the prefix. LSW ships no runtime.

WPF and WinForms run badly under Wine. The supported path is console
applications and service applications. Use `lsw verify --native` to get
the real Windows result.

### NativeAOT

`lsw build --aot` compiles C# with NativeAOT. The output is a native PE
binary: smaller, faster to start, with no CLR. The full `inspect`,
`audit`, and `compat` tools apply to it.

The .NET SDK does not permit cross-OS NativeAOT. LSW supplies the MSVC CRT
glue itself and links with `lld-link`. This works from Linux without a
Microsoft SDK. The host needs `clang` and `lld-link`. The scope is x86_64
console applications. `lsw dotnet doctor` shows the NativeAOT row.

## Zig

LSW detects `build.zig` automatically. `lsw build` runs `zig build` for
the Windows target of the environment.

## Prebuilt Windows libraries

`lsw deps add <name>` installs a mingw-w64 library from the MSYS2 package
repositories into the project `deps/` sysroot. The download is verified
against its SHA-256 checksum. LSW records the package in `[dependencies]`
in `lsw.toml` and pins it in `lsw.lock`. Builds find the include and
library paths automatically.

- `lsw deps list` shows the installed libraries.
- `lsw deps remove <name>` removes one.
- `lsw deps vendor <dir>` copies a prebuilt `include/`, `lib/`, `bin/`
  tree into the same sysroot. Use this for vcpkg or Conan exports.
