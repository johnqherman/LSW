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

## What LSW is

LSW is one command-line tool for Windows development on Linux. It controls
the tools that you already have. MinGW-w64 or Clang does the compilation.
Wine runs the result. LSW adds the parts that these tools do not have:

- Isolated Wine environments, one for each target.
- A declarative project file, `lsw.toml`.
- A lock file, `lsw.lock`, for reproducible builds.
- Deterministic path mapping between the Linux view and the Windows view of
  your project.

`lsw build` makes a genuine Windows PE binary from your CMake, Meson,
Cargo, Zig, Make, or .NET project. `lsw run` starts the binary through
Wine. `lsw test`, `lsw debug`, and `lsw package` complete the loop.

A test that passes under Wine is not a test that passes on Windows. LSW
keeps these two results separate at all times. Use `lsw verify --native` to
run your binaries on a real Windows host and to get the real result.

WSL puts a Linux userland on Windows. LSW points the other way: you develop
for the Windows target, and you do not leave Linux.

## Where to go

- **[Install](guide/install.md)** - install LSW and the host tools.
- **[Quickstart](guide/quickstart.md)** - from zero to a passed
  `lsw check` in five commands.
- **[Command reference](reference/commands.md)** - all commands and flags.
- **[Configuration](reference/configuration.md)** - the full `lsw.toml`
  schema and all `LSW_*` environment variables.
- **[Troubleshooting](troubleshooting.md)** - the full `LSW####` error
  catalogue and the usual first-run failures.
