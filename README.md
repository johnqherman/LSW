<img src="assets/logo.svg" alt="LSW" width="96" align="right"/>

# LSW - Linux Subsystem for Windows Development

[![CI](https://github.com/johnqherman/LSW/actions/workflows/ci.yml/badge.svg)](https://github.com/johnqherman/LSW/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/lsw.svg)](https://crates.io/crates/lsw)
[![docs](https://img.shields.io/badge/docs-johnqherman.github.io%2FLSW-teal.svg)](https://johnqherman.github.io/LSW/)
[![license](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)
[![msrv](https://img.shields.io/badge/rustc-1.85+-blue.svg)](Cargo.toml)

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

`lsw build` makes a genuine Windows PE binary from your CMake, Meson,
Cargo, Zig, Make, or .NET project. `lsw run` starts it through Wine.
`lsw test`, `lsw debug`, and `lsw package` complete the loop, and
`lsw verify --native` runs the result on a real Windows host - LSW never
reports a Wine pass as a Windows pass.

## Install

```
cargo install lsw && lsw install
```

You also need Wine, a MinGW-w64 or llvm-mingw toolchain, and your build
system. `lsw doctor` names anything missing.

## Documentation

**All documentation is at
[johnqherman.github.io/LSW](https://johnqherman.github.io/LSW/):**
installation, quickstart, guides for every workflow, the full command
reference, the `lsw.toml` schema, and the error catalogue.

Security model and vulnerability reporting: [SECURITY.md](SECURITY.md).
Contributing: [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0 OR MIT.
