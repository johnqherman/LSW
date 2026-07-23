# LSW for VS Code

Build, run, test, and debug native Windows applications from Linux through the
[LSW](https://github.com/johnqherman/LSW) CLI.

## Requirements

`lsw` on your `PATH` (`cargo install lsw && lsw install`) and a project with an
`lsw.toml`. The extension activates when the workspace contains one.

## Commands

- **LSW: Build** / **Run** / **Test** - run `lsw build|run|test` in the integrated terminal.
- **LSW: Verify on native Windows** - `lsw verify --native-windows` against the `[verify]` host.
- **LSW: Configure C/C++ IntelliSense** - writes `.vscode/c_cpp_properties.json` from `lsw ide env` (target, compiler, sysroot, include paths, defines).

## Debugging

Contributes an `lsw` debug type backed by `lsw dap` (Debug Adapter Protocol
over stdio). Example `launch.json` entry:

```json
{
  "type": "lsw",
  "request": "launch",
  "name": "Debug PE",
  "program": "${workspaceFolder}/build/app.exe"
}
```

## License

Apache-2.0 OR MIT.
