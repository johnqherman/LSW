# Languages and build systems

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
`lsw verify --native` for the real Windows verdict.

`lsw build --aot` compiles C# with NativeAOT to a native PE: smaller,
faster to start, no CLR, and the full `inspect`/`audit`/`compat` tooling
applies. The .NET SDK does not permit cross-OS NativeAOT; LSW supplies the
MSVC CRT glue itself and links with `lld-link`, so it works from Linux
without a Microsoft SDK (needs `clang` and `lld-link`; x86_64 console apps
only - `lsw dotnet doctor` shows the row).

**Zig** (`build.zig`) builds via `zig build` for the environment's Windows
target.

## Prebuilt Windows libraries

`lsw deps add <name>` installs mingw-w64 libraries (headers, import/static
libs, DLLs) from the MSYS2 repos into the project `deps/` sysroot,
sha256-verified and recorded in `[dependencies]` and `lsw.lock`. Builds pick
up the include and library paths automatically. `lsw deps vendor <dir>`
copies any prebuilt `include/`/`lib/`/`bin/` tree - a vcpkg or Conan
export, for example - into the same sysroot.
