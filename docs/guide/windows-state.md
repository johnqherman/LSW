# Shell, paths, and Windows state

## The two views of your project

Each environment mounts the project at `C:\src\<name>` on its drive C:.
The same files are visible from Linux at the project root. LSW maps paths
between the two views deterministically.

- `lsw path --to-windows <linux-path>` prints the Windows form.
- `lsw path --to-linux <windows-path>` prints the Linux form.

## Shells

`lsw shell` opens a Linux shell with the Windows-target variables set:
`CC`, `CXX`, `LSW_ENV`, `LSW_PROJECT`, `LSW_TARGET_FLAGS`, and
`WINEPREFIX`.

`lsw shell --windows` opens `cmd` (or `powershell.exe` when the prefix has
it) in `C:\src\<name>`.

`lsw exec <command>` runs one command. Use `--windows` or `--host` to
select the execution domain. `lsw run` selects the domain from the file
type: a PE binary runs through Wine, an ELF binary runs natively.

## Registry

Each environment has its own registry. `lsw registry` (alias `reg`)
manages it:

- `get <key> [value]` - read a key.
- `set <key> <value> <data> [--type string|dword|expand]` - write a value.
- `export <key> <file>` / `import <file>` - move keys through `.reg`
  files.
- `seed` - apply the `[[registry.seed]]` entries from `lsw.toml`. Use this
  to give tests a declarative starting state.
- `reset` - discard all registry state and rebuild the prefix defaults.

## Processes and services

- `lsw ps` lists the processes that run in the environment. Wine
  infrastructure is hidden. Add `--all` to show it.
- `lsw kill <pid>` sends SIGTERM to one process. `lsw kill --all` shuts
  the environment down.
- `lsw service create|start|stop|query|delete` manages Windows services
  in the environment. The `service` project template gives you a service
  skeleton to start from.

Note: the global `--env <name>` flag points these commands at a named
environment. With it, they work outside a project directory.

## The daemon and plugins

`lswd` is an optional background daemon, managed with
`lsw daemon start|status|stop`. It answers version and ping probes only.
Normal use does not need it.

`lsw plugin list` discovers `lsw-provider-*` executables on `PATH` and
shows their handshake information. The plugin surface is experimental.
Builds do not consult providers yet.
