# Editor integration

These are thin editor packages over the `lsw` CLI. The CLI stays the source of
truth. The packages start `lsw build|run|test|verify`, read `lsw ide env`
(JSON: target, compiler, sysroot, include paths, defines) to configure the
language tools, and start `lsw dap` as a Debug Adapter Protocol backend.

## VS Code (`vscode/`)

A TypeScript extension. It is on the Marketplace as
[`lsw.lsw`](https://marketplace.visualstudio.com/items?itemName=lsw.lsw).
To install it, use `code --install-extension lsw.lsw` or the Extensions view.
The source is `src/extension.ts`. `tsc` does the type checks. esbuild makes
the bundle `dist/extension.js`. The extension adds the commands
**LSW: Build / Run / Test / Verify** and **LSW: Configure C/C++ IntelliSense**
(this writes `.vscode/c_cpp_properties.json` from `lsw ide env`). It also adds
an `lsw` debug type that runs `lsw dap`.

```
cd editors/vscode
npm install
npm run build            # type-check happens via npm run typecheck
code --extensionDevelopmentPath=$PWD
```

To make a `.vsix`, use `vsce package` (`vscode:prepublish` does the type
checks and makes the bundle smaller first). To debug, add a `launch.json`
entry:

```json
{ "type": "lsw", "request": "launch", "name": "Debug PE", "program": "${workspaceFolder}/build/app.exe" }
```

## Neovim (`nvim/`)

A Lua plugin. It supplies `:LswBuild`, `:LswTest`, `:LswVerify`,
`:LswRun [program]`, `:LswConfigure` (this reads `lsw ide env` into
`vim.g.lsw_*`), and `:LswEnv`. If
[nvim-dap](https://github.com/mfussenegger/nvim-dap) is installed, the plugin
also adds an `lsw` adapter and a launch configuration.

Point your plugin manager at `editors/nvim`. Then:

```lua
require("lsw").setup({ path = "lsw" })
```

## JetBrains IDEs (`jetbrains/`)

`lsw-tools.xml` is an External Tools definition. Copy it to the `tools`
configuration directory of the IDE (for example
`~/.config/JetBrains/<Product><Version>/tools/`), or make the entries again
under **Settings > Tools > External Tools**. It adds the actions
**LSW Build / Test / Verify / Run** for the project directory.
