# Editor integration

Thin editor packages over the `lsw` CLI, which stays the source of truth. They
invoke `lsw build|run|test|verify`, consume `lsw ide env` (JSON: target,
compiler, sysroot, include paths, defines) to configure language tooling, and
launch `lsw dap` as a Debug Adapter Protocol backend.

## VS Code (`vscode/`)

A TypeScript extension. Source is `src/extension.ts`, type-checked with `tsc`
and bundled with esbuild to `dist/extension.js`. It contributes commands
**LSW: Build / Run / Test / Verify** and **LSW: Configure C/C++ IntelliSense**
(writes `.vscode/c_cpp_properties.json` from `lsw ide env`), plus an `lsw` debug
type that runs `lsw dap`.

```
cd editors/vscode
npm install
npm run build            # type-check happens via npm run typecheck
code --extensionDevelopmentPath=$PWD
```

Package it with `vsce package` to produce a `.vsix` (`vscode:prepublish`
type-checks and minifies first). Debug with a `launch.json` entry:

```json
{ "type": "lsw", "request": "launch", "name": "Debug PE", "program": "${workspaceFolder}/build/app.exe" }
```

## Neovim (`nvim/`)

A Lua plugin providing `:LswBuild`, `:LswTest`, `:LswVerify`, `:LswRun [program]`,
`:LswConfigure` (reads `lsw ide env` into `vim.g.lsw_*`), and `:LswEnv`. With
[nvim-dap](https://github.com/mfussenegger/nvim-dap) installed it also registers
an `lsw` adapter and launch configuration.

Point your plugin manager at `editors/nvim`, then:

```lua
require("lsw").setup({ path = "lsw" })
```

## JetBrains IDEs (`jetbrains/`)

`lsw-tools.xml` is an External Tools definition. Copy it to the IDE's `tools`
config directory (for example `~/.config/JetBrains/<Product><Version>/tools/`),
or recreate the entries under **Settings > Tools > External Tools**. It adds
**LSW Build / Test / Verify / Run** actions bound to the project directory.
