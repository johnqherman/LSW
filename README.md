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
lsw build                     # cross-compile to build/hello.exe (real PE)
lsw run build/hello.exe       # execute locally through the Wine runtime
lsw inspect build/hello.exe   # PE format, arch, subsystem, imports
lsw doctor                    # diagnose host / runtime / toolchain / project
lsw shell                     # Linux shell with Windows-target env exported
lsw shell --windows           # cmd.exe inside the environment
```

A binary produced by `lsw build` is a genuine Windows PE executable; running
it under LSW exercises the local compatibility runtime (Wine). LSW never
equates local runtime success with native Windows success.

## License

Apache-2.0 OR MIT.
