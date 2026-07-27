# Editor integration

- **VS Code**: `code --install-extension lsw.lsw` - build/test tasks with
  clickable errors, IntelliSense configuration from `lsw ide env`, and Wine
  debugging through `lsw dap`.
- **Neovim**: point your plugin manager at `editors/nvim` and call
  `require("lsw").setup()`.
- **JetBrains**: External Tools definitions in `editors/jetbrains`.

Details in
[editors/README.md](https://github.com/johnqherman/LSW/blob/main/editors/README.md).

Any other editor can integrate directly:

- `lsw ide env` prints compiler, flags, include paths, defines, and the
  Wine prefix as JSON.
- `lsw dap` is a Debug Adapter Protocol server on stdio.
- `lsw ide launch-config` writes a ready `.vscode/launch.json`.
