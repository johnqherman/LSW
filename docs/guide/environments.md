# Environments and reproducibility

An environment is an isolated Wine prefix plus a probed toolchain. Each
environment lives in `~/.local/share/lsw/environments/<name>/`. A project
selects its environment with `lsw use <name>`. The selection is recorded
in `lsw.toml`.

The global `--env <name>` flag overrides the selection for one command. It
also lets `ps`, `kill`, `registry`, and `service` work outside a project
directory.

## Manage environments

- `lsw env create <name> [--arch <arch>] [--toolchain <id>] [--sdk <name>]
  [--expose-home] [--force]` - create an environment.
- `lsw env list` - list them, with health status.
- `lsw env clone <src> <dst>` - fast reflink copy.
- `lsw env remove <name>` - delete an environment and its prefix.
- `lsw env provision winetricks <verbs...>` - run winetricks in the
  prefix. Examples: `dxvk`, `vkd3d`, `corefonts`.

## Move environments between machines

`lsw env export <name> <file.tar.zst>` packs an environment into one
archive. `lsw env import-archive <name> <file>` restores it. Use this to
move a prepared environment through a CI cache, instead of a `wineboot`
run on each job.

## The lock file

`lsw.lock` pins the build inputs:

- The toolchain: the SHA-256 checksum of the compiler.
- The runtime: the SHA-256 checksum of the Wine binary, and its version.
- The sysroot fingerprint.
- Each `[dependencies]` package: its version and its SHA-256 checksum.

When the environment drifts from the pins, the build fails with `LSW2006`.
To accept the new state, run `lsw build --update-lock`. To rebuild an
environment from the pins, run `lsw env restore <name>`. It verifies the
result against the lock file. Commit both `lsw.toml` and `lsw.lock` to
version control.
