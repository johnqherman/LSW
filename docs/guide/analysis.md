# Binary analysis

The analysis commands accept an explicit file. When you give no file, LSW
builds the project and uses its artifact.

## Examine a binary

`lsw inspect [pe]` shows the format, the machine, the subsystem, the entry
point, the sections, the embedded resources (manifest, version
information, icon), and the imports. For each imported DLL, it shows if
the DLL is available in the runtime.

`lsw audit [pe]` reports the security hardening of the binary: ASLR, DEP,
CFG, SafeSEH, and signing. Each check is enabled, disabled, or not
applicable to the target architecture. The baseline verdict needs ASLR and
DEP.

`lsw exports [dll]` lists the exported symbols of a DLL.

## Dependencies

`lsw deps tree [pe]` shows the transitive DLL tree of a binary. Each node
is resolved, system, or missing. The search covers the artifact directory,
the toolchain sysroot, and the prefix `system32`.

## Compare and measure

`lsw diff <a.exe> <b.exe>` compares two binaries: imports, exports,
sections, and size.

`lsw size [pe]` breaks the size down by section bucket: code, data,
resources, and others.

- `--baseline <pe>` compares against an earlier build.
- `--max-growth <pct>` fails the command when a bucket grows more than
  this percentage. Use this as a CI gate.

`lsw strings [file] [--min <n>]` lists the printable ASCII and UTF-16
strings in a file.

## Software bill of materials

`lsw sbom [pe]` writes a CycloneDX SBOM. The components are the imported
DLLs, the `[dependencies]` packages with their SHA-256 checksums from
`lsw.lock`, and the toolchain identity.

## Crash dumps

`lsw crash <dump.dmp>` decodes a Windows minidump. It shows the exception,
the crash address, the faulting module and offset, the thread, and the
platform. `lsw crash --force <pe>` launches the program under winedbg and
forces a dump without a crash.
