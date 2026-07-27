# Troubleshooting

Every LSW failure carries an `LSW####` code. `lsw explain <code>` prints the
same summary and fix as this page. Start with `lsw doctor`: it checks the
host, Wine, the cross toolchain, your build tools, and the active
environment, and names anything missing.

## The common first-run failures

**`LSW1501` - wine not found.** Install Wine from your package manager, or
point `LSW_WINE` at a wine binary that is not on `PATH` (WineHQ `/opt`
builds, Proton, Nix profiles).

**`LSW1403` - no working cross toolchain.** Install `mingw-w64` from your
package manager, or download an [llvm-mingw](https://github.com/mstorsjo/llvm-mingw)
release, extract it, and set `LSW_TOOLCHAIN_DIRS=/path/to/llvm-mingw/bin`.

**`LSW2011` - a build tool is missing.** LSW drives your build system; for
the default C/C++ path install `cmake` and `ninja`. `lsw doctor` lists
exactly which tool a project needs.

**My existing CMake project will not configure.** LSW hands CMake a cross
toolchain file with `CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER` /
`LIBRARY ONLY` / `INCLUDE ONLY`. `find_package` calls that expect host Linux
libraries will (correctly) stop finding them; vendor Windows builds of those
libraries with `lsw deps add`, or gate them out for the Windows target.

**A GUI test hangs in CI.** Use `lsw test --headless` (runs under a virtual
display via `xvfb-run`) and install `xvfb`.

## How LSW compares

| | LSW | cargo-xwin | Docker + Wine image | raw mingw + wine |
|---|---|---|---|---|
| Languages | C, C++, Rust, C#, Zig | Rust only | whatever the image has | anything, by hand |
| ABI | GNU default, MSVC with your SDK | MSVC | image-dependent | GNU |
| Runs the result | yes, isolated Wine prefixes | no | yes, in-container | by hand |
| Test/verify story | `lsw test`, `lsw check`, native `lsw verify` | none | ad hoc | ad hoc |
| Packaging/signing | zip, MSI, MSIX, sign | none | ad hoc | ad hoc |
| Reproducibility | `lsw.lock` pins toolchain + wine | Cargo.lock only | image digest | none |
| Isolation | prefixes + optional bwrap sandbox | n/a | container | none |

LSW does not replace your build system or toolchain; it configures the ones
you already have behind one CLI.

## Error code catalogue

### LSW0000

A command-line usage error (bad flags or arguments), not a build or environment failure.

**Fix:** run the command with --help to see its accepted flags and arguments

### LSW1001

A configuration or state file could not be read.

**Fix:** check that the path named in the error exists and is readable

### LSW1002

A configuration or state file could not be written.

**Fix:** check permissions and free space for the path named in the error

### LSW1003

A TOML file failed to parse.

**Fix:** fix the syntax error at the file and position named in the error

### LSW1004

Internal serialization of a configuration structure failed.

**Fix:** this is an LSW bug; please report it with the full error message

### LSW1005

No lsw.toml was found in this directory or any parent.

**Fix:** run `lsw init` to scaffold a project, or cd into an existing one

### LSW1006

The home directory could not be determined.

**Fix:** set $HOME, or run as a user that has a home directory

### LSW1007

The environment was created by a newer LSW than this build supports.

**Fix:** upgrade LSW, or recreate the environment with `lsw env create --force`

### LSW1201

A host path passed to path translation was not absolute.

**Fix:** canonicalize the path first, or join it onto the project root

### LSW1202

No path mapping covers the given path.

**Fix:** keep files under the project root or the environment's drive_c

### LSW1203

The string is not a drive-letter Windows path.

**Fix:** pass a form like C:\\dir\\file, as produced by `lsw path` or Windows tools

### LSW1204

The path contains a non-UTF-8 component.

**Fix:** rename the offending file or directory to valid UTF-8

### LSW1301

The file could not be read for PE inspection.

**Fix:** check that the file exists and is readable

### LSW1302

The file has an MZ header but is not a valid PE image.

**Fix:** the binary is corrupt or truncated; rebuild or re-download it

### LSW1303

The file is not a PE executable.

**Fix:** pass a Windows .exe or .dll, such as one produced by `lsw build`

### LSW1401

The requested toolchain provider is unavailable on this system.

**Fix:** install the provider named in the error, or choose another with --toolchain

### LSW1402

The toolchain provider failed its probe compile.

**Fix:** read the probe output in the error; the toolchain cannot produce PE binaries

### LSW1403

No toolchain provider produced a working Windows PE binary.

**Fix:** install llvm-mingw or mingw-w64, then re-run `lsw env create`

### LSW1404

An unknown toolchain provider was requested.

**Fix:** use a provider named in the error message, or omit --toolchain

### LSW1501

The wine runtime was not found on PATH.

**Fix:** install wine (e.g. `pacman -S wine` or `apt install wine`)

### LSW1502

Wine prefix initialization failed.

**Fix:** recreate the environment, or run `wineboot -u` with WINEPREFIX set to inspect

### LSW1503

The runtime could not spawn a program.

**Fix:** check that the binary named in the error exists and is executable

### LSW1504

Runtime execution failed.

**Fix:** read the detail in the error; adjust WINEDEBUG for more diagnostics

### LSW1505

A strict sandbox was requested but bubblewrap is not installed.

**Fix:** install bubblewrap, or drop --sandbox

### LSW1506

A virtual display was requested but xvfb-run is not installed.

**Fix:** install xvfb, or run with a real $DISPLAY

### LSW1507

The process is not running in this environment.

**Fix:** list processes with `lsw ps` to get a valid pid

### LSW1508

The LSW_WINE override does not point at an executable wine binary.

**Fix:** set LSW_WINE to a real wine executable, or unset it to use wine from PATH

### LSW2001

No active environment is selected for this project.

**Fix:** run `lsw use <name>` (or `lsw env create <name>` first)

### LSW2002

The named environment does not exist.

**Fix:** create it with `lsw env create <name>`, or list with `lsw env list`

### LSW2003

An environment with that name already exists.

**Fix:** remove it with `lsw env remove <name>`, or choose another name

### LSW2004

The target is not something LSW can execute.

**Fix:** pass a PE/ELF/script, or force a domain with --host or --windows

### LSW2005

The build command failed.

**Fix:** re-run `lsw build --verbose` and read the compiler output above

### LSW2006

Lsw.lock does not match the active environment.

**Fix:** refresh the pins with `lsw build --update-lock`, or `lsw env restore`

### LSW2007

No build system was detected.

**Fix:** add CMakeLists.txt, Cargo.toml, meson.build, build.zig, a .csproj, or set [build] command in lsw.toml

### LSW2008

The target os in lsw.toml is not supported.

**Fix:** set os = "windows"; LSW only targets Windows

### LSW2009

Project scaffolding failed.

**Fix:** read the detail in the error; check the directory is writable

### LSW2010

An io operation failed.

**Fix:** check the named path: existence, permissions, free space

### LSW2011

A required external tool was not found on PATH.

**Fix:** install the tool named in the error message

### LSW2012

An invalid environment or project name was given.

**Fix:** use a name without slashes, dots-only, or control characters

### LSW2013

The build produced a host binary instead of a Windows PE binary.

**Fix:** use the generated CMake toolchain, or make [build] honor CC/CXX/CFLAGS/LDFLAGS

### LSW2014

No tests were found to run.

**Fix:** add add_test(...) to CMakeLists.txt, or set [test] command in lsw.toml

### LSW2015

A registry operation failed.

**Fix:** check the key path (e.g. HKCU\\Software\\Example\\App) and the output above

### LSW2016

The process does not belong to this environment or already exited.

**Fix:** list this environment's processes with `lsw ps`

### LSW2017

The build directory was configured without the Windows test emulator.

**Fix:** remove the build/ directory and re-run `lsw test`

### LSW2018

Two build artifacts share the same file name.

**Fix:** rename a target, or package a single configuration

### LSW2019

An SDK with that name is already imported.

**Fix:** re-import with `lsw sdk import <name> --force`, or `lsw sdk remove <name>`

### LSW2020

The named SDK is not imported.

**Fix:** list imported SDKs with `lsw sdk list`

### LSW2021

An unsupported verification transport was configured.

**Fix:** set transport to "ssh", "winrm", or "https" in [verify]

### LSW2022

A provider plugin violated the plugin protocol.

**Fix:** update or fix the plugin named in the error

### LSW2023

The optional lsw daemon is not running.

**Fix:** start it with `lswd`; most commands work without the daemon

### LSW2024

An unsafe path or name was passed to native verification.

**Fix:** use a drive-letter path with segments of [A-Za-z0-9._+-] only

### LSW2025

Rust has no GNU-ABI Windows target for this arch.

**Fix:** use x86_64, x86, or aarch64 for Rust projects

### LSW2026

A Windows service operation failed.

**Fix:** read the operation detail in the error message

### LSW2027

The compatibility database could not be read or queried.

**Fix:** read the detail in the error message

### LSW2028

The debug adapter hit a protocol error.

**Fix:** restart the debug session; read the detail in the error message

### LSW2029

MSIX signing failed.

**Fix:** check the signing certificate and key; read the detail in the error

### LSW2030

An invalid [sandbox] network value was set.

**Fix:** use network = "host", "isolated", or "none"

### LSW2031

The crash dump could not be parsed.

**Fix:** pass a Windows minidump (.dmp), such as one written by the runtime

### LSW2032

The native import probe failed on the remote host.

**Fix:** check ssh connectivity to the host named in the error

### LSW2033

The package was not found in the mingw-w64 package set.

**Fix:** use the upstream library name (e.g. zlib, sqlite3, libpng)

### LSW2034

A download failed.

**Fix:** check network access and the URL in the error, then retry

### LSW2035

A downloaded file failed checksum verification.

**Fix:** retry the download; if it persists the mirror is serving a bad file

### LSW2036

An archive could not be unpacked.

**Fix:** retry the download; the archive may be corrupt

### LSW2037

No mingw-w64 package repository exists for this arch.

**Fix:** use an arch with a mingw-w64 repo, or vendor the dependency manually

### LSW2038

Cross-architecture execution needs a Wine build for the target arch.

**Fix:** set the env var named in the error to a matching wine (under qemu)

### LSW2039

Case = "strict" found case-insensitive file name collisions.

**Fix:** rename the colliding files, or set [filesystem] case = "native"

### LSW2040

The MSI failed install/uninstall verification in a scratch environment.

**Fix:** inspect the msiexec output in the error; rerun `lsw package --target msi --verify`

### LSW2041

C# NativeAOT cross-compilation prerequisites are missing.

**Fix:** needs x86_64 target, clang, lld-link, and a mingw-w64 sysroot; see the error detail

### LSW2042

The SDK import failed.

**Fix:** pass --from \<dir> pointing at an SDK directory; see the error detail

### LSW2043

A custom [filesystem] project_drive/mount_project was set but is not supported.

**Fix:** use project_drive = "C:" and mount_project = "/src" (the project mounts at C:\\src\\<name>)

### LSW2044

Env restore cannot reconstruct an SDK/MSVC toolchain (lsw.lock has no SDK identity).

**Fix:** re-import the SDK and recreate the environment with `lsw env create <name> --sdk <name>`

### LSW2045

The project or environment path resolves to a system directory, so strict sandbox isolation refuses to bind it.

**Fix:** move the project out of the filesystem root or a system path, or relax [sandbox] isolation

### LSW2046

The --system value is not a build system LSW knows.

**Fix:** use cmake, cargo, make, ninja, meson, zig, dotnet, or explicit with a [build] command

