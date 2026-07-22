# LSW - Linux Subsystem for Windows Development

LSW is a Linux-native development environment for building, running, and
inspecting Microsoft Windows applications without leaving Linux. It is
conceptually the inverse of WSL: where WSL hosts a Linux environment on
Windows, LSW makes Windows a first-class *target* from a Linux host.

LSW composes existing, mature technology - Wine as the execution runtime,
Clang/MinGW-w64 (GNU ABI) or clang-cl (MSVC ABI) as the cross toolchain,
CMake/Ninja/Cargo as build orchestrators - behind one coherent CLI with
isolated per-environment Wine prefixes, declarative `lsw.toml` project
configuration, `lsw.lock` reproducibility pinning, and deterministic
Linux<->Windows path mapping. C, C++, and Rust are first-class languages.

## Quickstart

```
lsw init hello && cd hello    # scaffold lsw.toml + CMake hello project
lsw env create win11-x64      # isolated Wine prefix + toolchain probe
lsw use win11-x64             # select the active environment
lsw build                     # cross-compile to build/hello.exe (real PE)
lsw run build/hello.exe       # execute locally through the Wine runtime
lsw test                      # run tests under the runtime (honest compat status)
lsw inspect build/hello.exe   # PE format, arch, subsystem, imports
lsw compat build/hello.exe    # measured compatibility report (imports + trace)
lsw trace build/hello.exe     # observe DLL loads + unsupported APIs
lsw debug build/hello.exe     # winedbg (or --gdb proxy for IDE attach)
lsw package                   # assemble dist/<name>-<arch>[.zip]
lsw doctor                    # diagnose host / runtime / toolchain / project
lsw shell                     # Linux shell with Windows-target env exported
lsw shell --windows           # cmd.exe inside the environment
```

### More commands

- **Execution / paths** - `lsw exec [--host|--windows] <cmd>`,
  `lsw path --windows|--linux`, `lsw run --sandbox strict <app.exe>`
  (bubblewrap kernel sandbox), `lsw run --headless <gui.exe>` (virtual X
  display for CI).
- **Environment state** - `lsw registry get|set|export|import|reset`,
  `lsw ps`, `lsw kill <pid>|--all`, `lsw service create|start|stop|query|delete`
  (Windows services via the prefix's `sc.exe`).
- **Compatibility** - `lsw compat --db <app.exe>` records into a persistent
  compatibility database; `lsw compat-query <dll|module!func>` looks it up.
- **Native verification** - `lsw verify --native-windows` builds, then runs the
  artifacts on a real Windows host over SSH (configured in `[verify]`), yielding
  an honest `WINDOWS_VERIFIED` / `WINDOWS_UNAVAILABLE` status distinct from the
  local Wine result.
- **Integration** - `lsw ide env` (JSON for editor plugins), `lsw dap` (a Debug
  Adapter Protocol server over stdio for IDEs), `lsw plugin list` (out-of-process
  `lsw-provider-*` JSON-RPC providers), `lswd` + `lsw daemon status|stop` (an
  optional caching daemon; not required for normal use).

## Packaging

`lsw package` assembles a distributable from the build output:

```
lsw package --target portable-directory   # dist/<name>-<arch>/
lsw package --target zip                   # + .zip
lsw package --target msi                   # Windows Installer (needs wixl/msitools)
lsw package --target msix                  # signed MSIX (needs zip + osslsigncode)
```

MSIX packages are built natively - LSW generates the `AppxManifest.xml`,
`AppxBlockMap.xml` (SHA-256 block hashes) and content types, zips the OPC
package, and signs it with [osslsigncode](https://github.com/mtrojnar/osslsigncode)
using a cached self-signed identity (`~/.local/share/lsw/msix/`). A self-signed
package installs only where its certificate is trusted (or in Windows developer
mode); as with `lsw verify`, actual Windows installation is not asserted here.

## Languages

**C / C++** build through CMake (auto-detected via `CMakeLists.txt`) or an
explicit `[build]` command in `lsw.toml`.

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
without touching the system mingw-w64 install. Each provider takes its sysroot
from the compiler's own location (`<root>/<triple>`), falling back to
`/usr/<triple>`.

```
export LSW_TOOLCHAIN_DIRS=/path/to/llvm-mingw/bin   # for aarch64
lsw env create arm64 --arch aarch64
lsw build                                           # -> build/app.exe (ARM64 PE)
```

Building an `aarch64` PE works on an `x86_64` host, but *running* it locally
does not: Wine cannot execute a foreign-architecture PE without CPU
translation, which lives outside LSW's core. `lsw run` fails
honestly ("Bad format") rather than pretending; verify ARM64 output with
`file` / `lsw inspect`, or on real hardware via `lsw verify --native-windows`.

A binary produced by `lsw build` is a genuine Windows PE executable; running
it under LSW exercises the local compatibility runtime (Wine). LSW never
equates local runtime success with native Windows success.

## Linking: static (default) or dynamic

By default LSW links the C/C++ runtime statically, so artifacts are
self-contained (they need only DLLs shipped with Windows 10+ - KERNEL32 + the
UCRT api-sets). Set `link = "dynamic"` under `[toolchain]` in `lsw.toml` to link
the shared mingw runtime instead; LSW then automatically deploys the runtime
DLLs the binary imports (e.g. `libstdc++-6.dll`, `libgcc_s_seh-1.dll`,
`libwinpthread-1.dll`, transitively) next to the artifact, so `lsw run` and
`lsw package` work without the build host.

```toml
[toolchain]
link = "dynamic"
```

## Notes

- `lsw shell --windows` opens `cmd` at the project's `C:\src\<name>` directory
  (via `cd /d`, which Wine keeps as `%CD%`). Non-interactive `lsw run` still
  inherits the host working directory, which Wine surfaces under `Z:\`; pass an
  explicit Windows working directory to a program if it matters.

## License

Apache-2.0 OR MIT.
