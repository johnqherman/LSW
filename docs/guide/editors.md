# Editor integration

## VS Code

Install the extension:

```
code --install-extension lsw.lsw
```

The extension gives you:

- Commands: Setup, Init, Build, Run, Test, Doctor, Verify.
- Tasks for build, test, check, and package, with a problem matcher, so
  build errors are clickable.
- IntelliSense configuration written from `lsw ide env`.
- Wine debugging through the `lsw dap` adapter. `lsw ide launch-config`
  writes a ready `.vscode/launch.json`.

## Neovim

Point your plugin manager at the `editors/nvim` directory of the
repository, then call `require("lsw").setup()`. This defines the `:Lsw*`
commands.

## JetBrains

Import the External Tools definitions from `editors/jetbrains`.

## Any other editor

Two stable interfaces cover the rest:

- `lsw ide env` prints the compiler, the flags, the include paths, the
  defines, and the Wine prefix, as JSON.
- `lsw dap` is a Debug Adapter Protocol server on stdio.
