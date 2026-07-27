# Install

## Install LSW

```
cargo install lsw && lsw install
```

`cargo install lsw` puts the `lsw` and `lswd` binaries on your `PATH`.
`lsw install` adds shell completions (bash, zsh, fish) and man pages. The
shell integration is optional. To install only the binaries, run
`cargo install lsw` alone.

## Install the host tools

LSW controls tools that are on your machine. It does not replace them. You
need:

- Wine, version 9 or later. `lsw doctor` shows a warning for older
  versions.
- A MinGW-w64 or llvm-mingw cross toolchain.
- Your build system.

For the default C/C++ path, install these packages:

```
# Ubuntu / Debian
sudo apt install wine wine64 mingw-w64 cmake ninja-build

# Fedora
sudo dnf install wine mingw64-gcc mingw64-gcc-c++ cmake ninja-build

# Arch
sudo pacman -S wine mingw-w64-gcc cmake ninja

# Nix (get the toolchain with: lsw toolchain install)
nix-shell -p wineWowPackages.stable cmake ninja
```

For other languages:

- Rust projects need `rustup` with the `x86_64-pc-windows-gnu` target.
- .NET projects need the `dotnet` SDK.
- Zig projects need `zig`.

Some features need more tools. Install them only when you use the feature:

| Feature | Tools |
|---------|-------|
| Headless GUI tests | `xvfb-run` |
| `lsw run --sandbox strict` | `bubblewrap` |
| `lsw package --target msi` | `msitools` (`wixl`) |
| `lsw package --target msix`, `lsw sign` | `zip`, `osslsigncode`, `openssl` |
| `lsw package --target nsis` | `makensis` |
| `lsw deps add` | `curl`, `tar` |
| Cross-family emulation | `qemu-user` |

## What LSW downloads

LSW ships only its own source and binaries. It does not ship Wine, MinGW,
or Microsoft SDK content. You supply those tools. LSW does not download
files without your command. Three commands download content, and only when
you run them:

- `lsw deps add` (MSYS2 package repositories)
- `lsw toolchain install` (llvm-mingw releases from GitHub)
- `lsw sdk acquire` (Microsoft SDK content through xwin)

See
[SECURITY.md](https://github.com/johnqherman/LSW/blob/main/SECURITY.md)
for the full list and the trust model.

## Check the setup

Run `lsw doctor`. It examines the host, Wine, the toolchain, your build
tools, and the active environment. It names each missing item.

If your Wine binary is not on `PATH`, set `LSW_WINE` to its location.
Examples: WineHQ builds in `/opt`, Proton, and Nix profiles.
