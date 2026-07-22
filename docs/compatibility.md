# Compatibility model

LSW treats Windows compatibility as **measured, not guaranteed**. A program that
runs under the local runtime (Wine) is never assumed to behave identically on
native Windows.

## Local inspection

- `lsw inspect <app.exe>`: PE format, architecture, subsystem, imported DLLs and
  their local availability.
- `lsw compat <app.exe>`: measured report combining imported DLLs, imported API
  function count, and a runtime trace of what actually loaded.
- `lsw trace <app.exe>`: DLL loads and unsupported-API messages observed under
  the runtime.

`lsw compat --db` records observations into a persistent compatibility database
queryable with `lsw compat-query <dll|module!func>`. `lsw compat --native` also
runs the binary on the configured `[verify]` Windows host and prints a real
Windows verdict next to the local (Wine) measurement.

## Native verification

`lsw verify --native-windows` builds and then runs the artifacts on a real
Windows host over SSH (configured in `[verify]`), yielding an honest
`WINDOWS_VERIFIED` / `WINDOWS_UNAVAILABLE` status that is kept distinct from the
local Wine result. Without a configured host the native result stays UNKNOWN;
LSW never reports a Wine pass as a Windows pass. Failing artifacts report their
Windows exit code, and common crash codes are decoded (access violation, a
missing DLL, wrong architecture, and so on).

When the `[verify]` host has WER LocalDumps configured with `dump_dir` set, a
crashing artifact's minidump is pulled back over scp and decoded: the verify
output names the exception, the faulting module and offset, and the crash
address. Any minidump can also be decoded directly with `lsw crash <dump>`.
