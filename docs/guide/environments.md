# Environments and reproducibility

An environment is an isolated Wine prefix plus a probed toolchain, stored
in `~/.local/share/lsw/environments/<name>/`. Projects select one with
`lsw use <name>` (recorded in `lsw.toml`), or per invocation with the
global `--env <name>` flag - which also lets `ps`, `kill`, `registry`, and
`service` run outside a project directory.

- `lsw env create <name> [--arch <arch>] [--toolchain <id>] [--sdk <name>]`
- `lsw env clone <src> <dst>` - fast reflink copy
- `lsw env restore <name>` - rebuild from `lsw.lock` and verify it matches
  the pins
- `lsw env export <name> <file.tar.zst>` / `lsw env import-archive` - move
  whole environments between machines (CI cache handoff)
- `lsw env provision winetricks <verbs...>` - dxvk, corefonts, anything
  winetricks knows

## lsw.lock

`lsw.lock` pins the toolchain (sha256 of the compiler), the runtime (sha256
of the wine binary and its version), the sysroot fingerprint, and every
`[dependencies]` package with its version and sha256. Builds fail with
`LSW2006` when the environment drifts from the pins;
`lsw build --update-lock` re-pins deliberately. Commit both `lsw.toml` and
`lsw.lock`.

## Windows state

Each prefix has its own registry and drive C:. `lsw registry
get|set|seed|export|import|reset` manages it; `[[registry.seed]]` entries
in `lsw.toml` give tests a declarative starting state. `lsw ps`, `lsw
kill`, and `lsw service create|start|stop|query|delete` manage runtime
processes and Windows services inside the environment.
