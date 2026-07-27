# Sandboxing and security

The Wine prefix is a **compatibility boundary, not a security boundary**. By
default `lsw run` executes a program with your Linux privileges, and
programs can reach the host filesystem through Wine's `Z:` drive. The
Windows user profile hides your host home directory unless the environment
was created with `--expose-home`.

For real isolation, `lsw run --sandbox strict` runs the program under
bubblewrap: read-only system directories, masked home, only the environment
and project writable, with optional CPU/memory rlimits and network modes
(`host`, `isolated` NAT via pasta, or `none`) from `[sandbox]` in
`lsw.toml`.

The full threat model - including every network download LSW can make on
request and the local executables it will run - is in
[SECURITY.md](https://github.com/johnqherman/LSW/blob/main/SECURITY.md),
along with the private vulnerability reporting channel.
