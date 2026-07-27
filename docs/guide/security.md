# Sandboxing and security

The Wine prefix is a compatibility boundary. It is not a security
boundary.

By default, `lsw run` executes a program with your Linux privileges. The
program can reach the host filesystem through Wine's `Z:` drive. The
Windows user profile hides your host home directory, unless you created
the environment with `--expose-home`.

Treat `lsw run <app.exe>` like a native Linux program that you downloaded.
Run only the binaries that you trust.

## The strict sandbox

For real isolation, run `lsw run --sandbox strict <app.exe>`. It uses
bubblewrap:

- The system directories are read-only.
- The home directory is masked.
- Only the environment and the project are writable.

The `[sandbox]` section in `lsw.toml` adds limits:

```toml
[sandbox]
network     = "none"   # host | isolated (NAT via pasta) | none
cpu_seconds = 30
memory_mb   = 2048
```

## Network and local execution

LSW does not download files without your command. Three commands download
content: `lsw deps add`, `lsw toolchain install`, and `lsw sdk acquire`.
`lsw plugin list` executes `lsw-provider-*` binaries from your `PATH`.

The full threat model and the private vulnerability-report channel are in
[SECURITY.md](https://github.com/johnqherman/LSW/blob/main/SECURITY.md).
