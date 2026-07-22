# Security model

LSW runs Windows programs on Linux through Wine. Understand what is and is not a
security boundary before running untrusted binaries.

## The Wine prefix is a compatibility boundary, not a security boundary

Each environment has its own Wine prefix, which isolates Windows *state*
(registry, drive C:, installed components) from other environments. It does
**not** contain a Windows program the way a sandbox does. By default a program
run with `lsw run` executes with your normal Linux user privileges and can:

- read and write the host filesystem through Wine's `Z:` drive (mapped to `/`);
- reach the network;
- read your environment variables.

Treat `lsw run <app.exe>` as equivalent to running any native Linux program you
downloaded: only do it with binaries you trust.

## Reducing exposure

- **Host home is hidden by default.** `lsw env create` replaces the Wine user
  profile links that point into your home directory with empty directories.
  Pass `--expose-home` to keep them if a program genuinely needs them.
- **Strict sandbox.** `lsw run --sandbox strict <app.exe>` runs the program
  inside a bubblewrap kernel sandbox: read-only system directories, the home
  directory masked, and only the environment and project directories writable.
  Network is dropped unless `[sandbox] network = "host"`. This is the real
  isolation boundary; use it for untrusted binaries.
- **No network.** Combine the strict sandbox with `network = "none"` (the
  default when sandboxed) to cut off outbound traffic.

## What LSW never does

- It never redistributes proprietary Microsoft SDK, runtime, or OS content; you
  supply those with `lsw sdk import`.
- It never claims a program that runs under Wine will behave identically on
  native Windows. Use `lsw verify --native-windows` for a real Windows result.

## Reporting

This is a personal project; open an issue on the repository for security
concerns.
