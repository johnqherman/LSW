# Toolchains

LSW produces real Windows PE binaries with a cross toolchain selected at
`lsw env create` time and recorded in the environment manifest.

## Providers

- **llvm-mingw** (preferred) and **mingw-gcc**: GNU-ABI targets. Discovered on
  `$PATH` and in `$LSW_TOOLCHAIN_DIRS` (colon-separated), so a self-contained
  llvm-mingw can be used without touching the system install.
- **clang-cl** (`--sdk <name>`): MSVC-ABI targets against a user-imported
  Windows SDK. LSW never ships SDK content.

The sysroot is taken from the compiler's own location (`<root>/<triple>`),
falling back to `/usr/<triple>`.

## Linking

Static by default (self-contained artifacts). Set `link = "dynamic"` under
`[toolchain]` in `lsw.toml` to link the shared MinGW runtime; LSW then deploys
the imported runtime DLLs next to the artifact.

## Target API version

`[target] api = "win10"` (also `win7`, `win8`, `win8.1`, `win11`, `vista`,
`winxp`) threads `-D_WIN32_WINNT`, `-DWINVER`, and `-DNTDDI_VERSION` into the
compile flags so headers expose the right API surface.

## Build systems

`lsw build` auto-detects CMake, Cargo, Meson, Ninja, and Make (or an explicit
`[build] command`). All receive the cross `CC`/`CXX`/`CFLAGS`/`CXXFLAGS`/
`LDFLAGS`; CMake and Meson also get generated cross/toolchain files.
