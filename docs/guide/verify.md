# Native Windows verification

A pass under Wine is not a pass on Windows. `lsw verify` gives the real
answer. It builds the project, copies the binaries to a real Windows host,
runs them there, and reports one of two results:

- `WINDOWS_VERIFIED` - the binaries ran on Windows.
- `WINDOWS_UNAVAILABLE` - no host was reachable. This is not a failure of
  your binary. It is the absence of a verdict.

## Configure the host

Add a `[verify]` section to `lsw.toml`:

```toml
[verify]
transport     = "ssh"            # ssh | winrm | https
host          = "user@win-host"
identity_file = "~/.ssh/lsw_verify"
```

- `ssh` needs an OpenSSH server on the Windows host.
- `winrm` and `https` (WinRM over TLS) read the password from the
  `LSW_WINRM_PASSWORD` environment variable.
- `remote_dir` sets the remote work directory. `dump_dir` sets the remote
  crash-dump directory. Both have transport defaults.

## What uses the host

- `lsw verify [--native]` - run the artifacts and report the verdict.
- `lsw compat --native` - add the real Windows verdict to a compatibility
  report.
- `lsw debug --native <pe>` - run under cdb for a real backtrace,
  `!analyze -v` triage, or an interactive session. This path needs the
  `ssh` transport.

## Reproducibility verification

`lsw verify --reproducible [artifact]` does not use the Windows host. It
builds the project two times and makes sure that the artifacts are
byte-identical. On a mismatch, it reports the sections that diverge.
