# LSW

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

## Where to go

- **[Install](guide/install.md)** - `cargo install lsw`, host requirements
  per distro, and what LSW will never download behind your back.
- **[Quickstart](guide/quickstart.md)** - from an existing project or from
  nothing to a passing `lsw check` in five commands.
- **[Command reference](reference/commands.md)** - every command, flag, and
  its semantics.
- **[Configuration](reference/configuration.md)** - the full `lsw.toml`
  schema and every `LSW_*` environment variable.
- **[Troubleshooting](troubleshooting.md)** - the complete `LSW####` error
  catalogue, common first-run failures, and how LSW compares to
  cargo-xwin and Docker+Wine.
