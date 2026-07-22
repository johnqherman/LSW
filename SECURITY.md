# Security model

LSW runs Windows programs on Linux through Wine. Before you run binaries that
you do not trust, know what is a security boundary and what is not.

## The Wine prefix is a compatibility boundary, not a security boundary

Each environment has its own Wine prefix. The prefix isolates the Windows
state (registry, drive C:, installed components) from the other environments.
It does **not** contain a Windows program in the way that a sandbox does. By
default, a program that you start with `lsw run` runs with your normal Linux
user privileges. The program can:

- read and write the host filesystem through the `Z:` drive of Wine (a map of `/`);
- get access to the network;
- read your environment variables.

Think of `lsw run <app.exe>` as the same as a native Linux program that you
downloaded: run only the binaries that you trust.

## How to decrease the exposure

- **The host home directory is hidden by default.** `lsw env create` replaces
  the Wine user profile links that point into your home directory with empty
  directories. If a program really needs those links, use `--expose-home` to
  keep them.
- **The strict sandbox.** `lsw run --sandbox strict <app.exe>` runs the
  program in a bubblewrap kernel sandbox. The system directories are
  read-only. The home directory is masked. Only the environment and the
  project directories are writable. The sandbox removes the network unless
  `[sandbox] network = "host"` is set. This is the real isolation boundary.
  Use it for binaries that you do not trust.
- **No network.** Use the strict sandbox together with `network = "none"` (the
  default in the sandbox) to stop outbound traffic.

## What LSW does not do

- LSW does not redistribute proprietary Microsoft SDK, runtime, or OS content.
  You supply that content with `lsw sdk import`.
- LSW does not tell you that a program that runs under Wine has the same
  behavior on native Windows. Use `lsw verify --native-windows` to get a real
  Windows result.

## How to report a vulnerability

Report security vulnerabilities privately, not through public issues. Use the
private vulnerability report function of GitHub on this repository:
**Security > Advisories > Report a vulnerability**
(https://github.com/johnqherman/LSW/security/advisories/new).

Include the LSW version (`lsw --version`), the applicable command or code
path, and steps to cause the problem again, if possible. You will get an
acknowledgement and a schedule for a repair or mitigation. Give a sufficient
time window for a repair before a public disclosure.
