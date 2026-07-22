# Sandboxing and isolation

See `SECURITY.md` for the threat model. Summary: the Wine prefix is a
compatibility boundary, not a security boundary. Real isolation comes from the
strict sandbox.

## Default run

`lsw run <app.exe>` executes with your Linux user privileges. Windows programs
can reach the host filesystem through Wine's `Z:` drive. Your host home is
hidden from the Windows user profile unless the environment was created with
`--expose-home`.

## Strict sandbox

```
lsw run --sandbox strict <app.exe>
```

Wraps the process in a bubblewrap namespace: read-only `/usr` and `/etc`, a
tmpfs home, and only the environment and project directories writable. Network
is dropped unless the project sets `[sandbox] network = "host"`.

```toml
[sandbox]
network = "host"   # or omit for no network under the sandbox
```

## Headless execution

`lsw run --headless <gui.exe>` and `lsw test --headless` run GUI programs under
a private Xvfb display, for CI without a real `$DISPLAY`.
