# Environments

An LSW environment is an isolated Windows-target development context: a Wine
prefix plus a resolved toolchain and runtime, stored under
`~/.local/share/lsw/environments/<name>/`.

## Lifecycle

```
lsw env create win11-x64 --arch x86_64   # initialize a prefix + probe a toolchain
lsw use win11-x64                        # select it for the current project
lsw env list                             # show environments and health
lsw env restore win11-x64                # rebuild from lsw.lock and verify pins
lsw env remove win11-x64                 # delete it
```

`lsw env create` initializes the Wine prefix (`wineboot -u`), provisions a clean
`C:\users\lsw` profile, and by default hides your host home directory from
Windows programs (pass `--expose-home` to keep it visible). Use `--arch` for the
target architecture and `--sdk <name>` to build against an imported Windows SDK
(clang-cl / MSVC ABI) instead of MinGW.

## Layout

```
environments/<name>/
  env.toml          # resolved toolchain + runtime manifest (format-versioned)
  prefix/           # the Wine prefix (drive_c, registry, users/lsw)
  logs/
  toolchain.cmake   # generated cross toolchain file
```

Sibling managed directories under the data dir: `sysroots/`, `runtimes/`,
`toolchains/`, `packages/`.

## Reproducibility

`lsw.lock` pins the toolchain, runtime, and sysroot by version and SHA-256.
`lsw env restore` recreates an environment from those pins and fails if the
host's components no longer match, which is what makes CI reproducible.
