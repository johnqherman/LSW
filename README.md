# LSW - Linux Subsystem for Windows Development

LSW is a Linux-native development environment for building, running, inspecting,
debugging, packaging, and verifying Microsoft Windows applications without
leaving Linux. It is conceptually the inverse of WSL: where WSL runs a Linux
environment on Windows, LSW builds, runs, and ships Windows software on Linux.

LSW composes existing, mature technology - Wine as the execution runtime,
Clang/MinGW-w64 as the cross toolchain, CMake, Meson, Cargo, Zig, .NET, and more as build orchestrators -
behind one coherent CLI with isolated per-environment Wine prefixes,
declarative `lsw.toml` project configuration, `lsw.lock` reproducibility
pinning, and deterministic Linux<->Windows path mapping.

## Quickstart

```
lsw init hello && cd hello    # scaffold lsw.toml + CMake hello project
lsw env create win11-x64      # isolated Wine prefix + toolchain probe
lsw use win11-x64             # select the active environment
lsw build                     # cross-compile to build/hello.exe (real PE)
lsw run build/hello.exe       # execute locally through the Wine runtime
lsw test                      # run tests under the runtime (honest compat status)
lsw inspect build/hello.exe   # PE format, arch, subsystem, imports
lsw trace build/hello.exe     # observe DLL loads + unsupported APIs
lsw debug build/hello.exe     # winedbg (or --gdb proxy for IDE attach)
lsw package                   # assemble dist/<name>-<arch>[.zip]
lsw doctor                    # diagnose host / runtime / toolchain / project
lsw shell                     # Linux shell with Windows-target env exported
lsw shell --windows           # cmd.exe inside the environment
```

Additional commands: `lsw exec [--host|--windows] <cmd>`, `lsw path
--windows|--linux`, `lsw registry get|set|export|import|reset`, `lsw ps`,
`lsw kill <pid>|--all`, `lsw ide env` (JSON for editor plugins). Windows
execution can be locked down with `lsw run --sandbox strict <app.exe>`
(bubblewrap kernel sandbox). Environments target `x86_64` (default) or `x86`
via `lsw env create --arch x86`.

A binary produced by `lsw build` is a genuine Windows PE executable; running
it under LSW exercises the local compatibility runtime (Wine). LSW never
equates local runtime success with native Windows success.

## Known limitations

- The project is reachable at `C:\src\<name>` inside every environment
  (forward mapping via a prefix symlink), but Wine derives a process's
  *working directory* from the kernel-resolved Unix path, so `cd` inside a
  Windows shell shows the host path under `Z:\` rather than `C:\src\<name>`.
- Artifacts are linked with static runtimes (`-static`); they depend only on
  DLLs shipped with Windows 10+ (KERNEL32 + UCRT api sets).

## License

Apache-2.0 OR MIT.
