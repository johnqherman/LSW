# Quickstart

## Start in an existing project

LSW detects CMake, Meson, Cargo, Zig, Make, and .NET projects.

1. Go to the project directory.
2. Run `lsw setup`. It detects the project, creates a default environment,
   and writes `lsw.toml`.
3. Run `lsw build`. It cross-compiles to `build/<name>.exe`, a real PE
   binary.
4. Run `lsw run`. It builds and runs the binary through Wine.
5. Run `lsw check`. It validates the full pipeline: configuration,
   environment, build, Wine execution, DLL resolution, and PE hardening.

```
lsw setup
lsw build
lsw run
lsw check
lsw package
```

## Start from nothing

1. Run `lsw init hello`. It scaffolds `lsw.toml` and a CMake hello
   project.
2. Go into the new directory.
3. Run `lsw setup`, then `lsw check`.

```
lsw init hello && cd hello
lsw setup
lsw check
```

`lsw init --template <name>` selects the scaffold:

| Template | Result |
|----------|--------|
| `console` | C console program (default) |
| `cpp` | C++ console program |
| `gui` | WinMain window program |
| `dll` | Shared library |
| `service` | Windows service skeleton |

Complete example projects are in
[`examples/`](https://github.com/johnqherman/LSW/tree/main/examples). Copy
one, or run it in place.

## The next commands

- `lsw test` - run the test suite under Wine.
- `lsw inspect` and `lsw audit` - examine the built PE binary.
- `lsw doctor` - examine the host setup.
- `lsw watch` - build again on each source change.
- `lsw verify --native` - run the binaries on a real Windows host.

For more environments, other architectures, and the MSVC ABI, use
`lsw env create` and `lsw use` directly. `lsw setup` is a shortcut over the
same machinery.

## The command map

```
Getting started:  setup  init  doctor  check
Build / run:      build  run  exec  test  shell  watch  clean
Analysis:         inspect  audit  exports  deps  diff  size  strings  sbom  crash
Compatibility:    compat  compat-query  trace
Debugging:        debug  dap
Ship:             package  sign  verify  ci
Environments:     env  use  registry  ps  kill  service  sdk  toolchain  path
Languages:        rust  dotnet
Tooling:          ide  plugin  daemon  config  explain  completions  man  install
```

See the [command reference](../reference/commands.md) for all flags. Most
report commands accept `--format json`. When a command fails with an
`LSW####` code, run `lsw explain <code>`, or see
[troubleshooting](../troubleshooting.md).
