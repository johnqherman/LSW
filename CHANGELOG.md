# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

Initial release. LSW is a Linux-native environment for building, running,
inspecting, debugging, packaging, and verifying Windows applications.

### Added

- Cross-compilation to Windows PE via LLVM/Clang + MinGW-w64 (GNU ABI) and
  clang-cl (MSVC ABI); C, C++, and Rust as first-class languages.
- Build-system detection and driving: CMake, Meson, Ninja, Make, Cargo, Zig
  (`build.zig`), and .NET (`.csproj`/`.sln`).
- Isolated per-environment Wine prefixes, declarative `lsw.toml`, `lsw.lock`
  reproducibility pinning, and deterministic Linux/Windows path mapping.
- Target architectures `x86_64`, `x86`, `aarch64`, `armv7`, `arm64ec`, with
  qemu user-mode emulation for running cross-architecture PEs.
- Binary analysis: `inspect`, `audit`, `exports`, `deps tree`, `crash` (minidump
  decode), `diff`, `strings`, `sbom`, `sign`.
- Dependency management: `deps add|remove|list` for prebuilt mingw-w64 libraries.
- Compatibility measurement: `compat` (local + native capability probe),
  `compat-query`, `trace`, and a persistent compatibility database.
- Debugging: winedbg, a gdb-remote stub, a Debug Adapter Protocol server, and
  `debug --native` (cdb backtrace on a real Windows host).
- Native verification over SSH, WinRM, and WinRM-over-TLS, with crash-dump
  collection.
- Packaging to zip, portable directory, MSI, and MSIX.
- Sandboxing (bubblewrap), registry management, process management, services,
  an optional daemon, and an out-of-process provider plugin protocol.
- Editor integrations (VS Code, Neovim, JetBrains) under `editors/`.

[Unreleased]: https://github.com/johnqherman/LSW/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/johnqherman/LSW/releases/tag/v0.1.0
