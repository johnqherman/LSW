# Dependency and redistribution audit

LSW is distributed under Apache-2.0 OR MIT and must not ship anything that
conflicts with that or with third-party licensing.

## Rust dependencies

All crate dependencies are permissively licensed. The policy is enforced by
`cargo-deny` using `deny.toml` at the repository root:

```
cargo install cargo-deny
cargo deny check
```

`cargo deny check` gates licenses (allow-list in `deny.toml`), security
advisories, yanked crates, and source registries. Run it before every release.

## What LSW redistributes

LSW ships only its own Rust source and the compiled `lsw` / `lswd` binaries. It
does **not** bundle or download:

- the Microsoft Windows SDK, CRT, or MSVC toolchain (user supplies via
  `lsw sdk import`);
- Wine, MinGW-w64, or llvm-mingw binaries (installed by the user's package
  manager or extracted from upstream releases);
- any Windows OS image, redistributable (vcredist, DirectX, .NET), or font.

Anything proprietary stays on the user's machine and is referenced by path,
never copied into an LSW artifact.

## Produced binaries

A binary built by `lsw build` links against MinGW-w64 / user-supplied SDK import
libraries per that toolchain's license. Static linking of the MinGW runtime is
covered by the GCC runtime library exception; dynamic linking bundles the
runtime DLLs from the toolchain the user installed.
