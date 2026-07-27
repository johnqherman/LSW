# Install

```
cargo install lsw && lsw install
```

`cargo install lsw` puts the `lsw` and `lswd` binaries on your `PATH`.
`lsw install` adds shell completions (bash, zsh, fish) and man pages. The
shell integration is optional; skip it with plain `cargo install lsw`.

## Host requirements

LSW drives tools that are already on your machine: Wine (9+ recommended,
`lsw doctor` warns on older), a MinGW-w64 or llvm-mingw cross toolchain, and
your build system. For the default C/C++ path that means:

```
# Ubuntu / Debian
sudo apt install wine wine64 mingw-w64 cmake ninja-build

# Fedora
sudo dnf install wine mingw64-gcc mingw64-gcc-c++ cmake ninja-build

# Arch
sudo pacman -S wine mingw-w64-gcc cmake ninja

# Nix (toolchain via lsw toolchain install or LSW_TOOLCHAIN_DIRS)
nix-shell -p wineWowPackages.stable cmake ninja
```

Rust projects need `rustup` with the `x86_64-pc-windows-gnu` target; .NET
projects need the `dotnet` SDK; Zig projects need `zig`. Optional features
have their own tools: `xvfb-run` (headless GUI tests), `bubblewrap`
(`--sandbox strict`), `msitools`/`wixl` (MSI), `makensis` (NSIS), `zip`,
`osslsigncode`, `openssl` (MSIX and signing), `curl` + `tar`
(`lsw deps add`), and `qemu-user` (cross-family emulation).

LSW ships only its own source and binaries - no Wine, no MinGW, no Microsoft
SDK or redistributable content. You supply those; nothing is downloaded
behind your back, and every download LSW can make on request is documented
in [SECURITY.md](https://github.com/johnqherman/LSW/blob/main/SECURITY.md).
`lsw doctor` checks the whole setup and names anything missing. If your
Wine is not on `PATH` (WineHQ `/opt` builds, Proton, Nix profiles), point
`LSW_WINE` at the binary.
