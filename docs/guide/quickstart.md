# Quickstart

In an existing project (CMake, Meson, Cargo, Zig, Make, or .NET):

```
lsw setup                     # detect project, create env, write lsw.toml
lsw build                     # cross-compile to build/<name>.exe (real PE)
lsw run                       # build + run the executable through Wine
lsw check                     # build, wine execution, deps, hardening
lsw package                   # assemble dist/<name>-<arch>[.zip]
```

Starting from nothing:

```
lsw init hello && cd hello    # scaffold lsw.toml + CMake hello project
lsw setup                     # create + wire the default environment
lsw check                     # validate the whole pipeline
```

`lsw init --template console|cpp|gui|dll|service` picks the scaffold.

Useful next commands: `lsw test` (test suite under Wine), `lsw inspect` /
`lsw audit` (PE analysis), `lsw doctor` (host diagnosis), `lsw watch`
(rebuild on change), `lsw verify --native` (real Windows verdict).

Multiple environments, other architectures, and the MSVC ABI use
`lsw env create` and `lsw use` directly; `lsw setup` is a convenience over
the same machinery.

Complete example projects live in
[`examples/`](https://github.com/johnqherman/LSW/tree/main/examples) - copy
one or run it in place.

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

Full flags and semantics: [command reference](../reference/commands.md).
Most report commands take `--format json`. If a command fails with an
`LSW####` code, `lsw explain <code>` describes it; the same catalogue is in
[troubleshooting](../troubleshooting.md).
