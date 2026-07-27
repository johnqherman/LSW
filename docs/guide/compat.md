# Compatibility and tracing

Wine does not implement all of the Windows API. These commands measure
what your binary uses and what the runtime supports. The results are
scoped: a local number describes the Wine runtime, not native Windows.

## Measure compatibility

`lsw compat [pe]` builds the project, runs the binary, and produces a
measured report:

- The imported DLLs and the imported API functions.
- The runtime trace of the execution.
- The API functions that the runtime did not support.

Options:

- `--db` - record the results in the local compatibility database.
- `--native` - also run the binary on the `[verify]` Windows host and add
  the real Windows verdict.

## Query past results

`lsw compat-query <dll>` or `lsw compat-query <module!function>` queries
the compatibility database. The database is local to your machine, in
`~/.local/share/lsw/compat-db.json`. LSW never uploads it.

## Trace an execution

`lsw trace [pe]` runs the binary and shows what happened:

- The DLL loads.
- The registry and filesystem access.
- The unsupported API calls.
- An event timeline.

Options:

- `--relay` - include the full API relay log. This is verbose.
- `--filter <glob>` - limit the events to matching names.
- `--format csv` or `--format json` - machine-readable timeline output.
