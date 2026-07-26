# Configuration reference

## `lsw.toml`

One file per project, discovered by walking up from the working directory.
A generated manifest contains only `[project]`; every other section is
optional. Unknown keys are hard errors, so typos fail loudly instead of
being ignored. Commit `lsw.toml` and `lsw.lock` to version control.

### `[project]`

| key | required | meaning |
|-----|----------|---------|
| `name` | yes | project name; also the artifact and package base name |

### `[target]`

| key | default | meaning |
|-----|---------|---------|
| `arch` | `x86_64` | `x86_64`, `x86`, `aarch64`, `armv7`, or `arm64ec`; `lsw setup` creates the default environment for this arch |
| `api` | none | minimum Windows API level (`win7`, `win8`, `win10`, `win11`); sets `_WIN32_WINNT`, `WINVER`, `NTDDI_VERSION` |
| `os` | `windows` | only `windows` is accepted |

### `[toolchain]`

| key | default | meaning |
|-----|---------|---------|
| `link` | `static` | `static` links the C/C++ runtime into the artifact; `dynamic` deploys the mingw runtime DLLs next to it |
| `aot` | `false` | compile C# with NativeAOT (native PE, no CLR) |
| `provider` | none | reserved; toolchain selection lives on the environment (`lsw env create --toolchain`) |
| `version` | none | reserved; pinning lives in `lsw.lock` |

### `[runtime]`

| key | default | meaning |
|-----|---------|---------|
| `provider` | `wine` | reserved; the runtime is resolved per environment |
| `version` | none | reserved; pinning lives in `lsw.lock` |

### `[environment]`

| key | default | meaning |
|-----|---------|---------|
| `name` | none | the active environment; written by `lsw use` and `lsw setup` |

### `[filesystem]`

| key | default | meaning |
|-----|---------|---------|
| `project_drive` | `C:` | only `C:` is supported today |
| `mount_project` | `/src` | only `/src` is supported today; the project mounts at `C:\src\<name>` |
| `case` | `native` | `strict` turns case-insensitive filename collisions into build errors |

### `[build]` / `[test]`

| key | meaning |
|-----|---------|
| `command` | explicit argv, e.g. `["make", "-f", "windows.mk"]`; skips build-system detection. The command runs with the cross `CC`, `CXX`, `CFLAGS`, `LDFLAGS` exported |

### `[env]`

```toml
[env.vars]           # extra Windows environment variables for run/exec/test
RUST_LOG = "debug"
[env.secret]         # inject a host variable by name; the value stays out of the manifest
API_TOKEN = "HOST_API_TOKEN"
```

### `[[registry.seed]]`

Registry values applied by `lsw registry seed`:

```toml
[[registry.seed]]
key   = "HKCU\\Software\\Hello"
name  = "FirstRun"
value = "1"
type  = "dword"      # string (default) | dword | expand
```

### `[sandbox]`

| key | default | meaning |
|-----|---------|---------|
| `network` | `host` | `host`, `isolated` (NAT via pasta), or `none` |
| `cpu_seconds` | none | rlimit for `lsw run --sandbox strict` |
| `memory_mb` | none | rlimit for `lsw run --sandbox strict` |

### `[verify]`

| key | default | meaning |
|-----|---------|---------|
| `transport` | `ssh` | `ssh`, `winrm`, or `https` |
| `host` | none | native Windows verification host, e.g. `user@win-host` |
| `identity_file` | none | ssh identity for the verification host |
| `remote_dir` | transport default | remote directory artifacts are copied to |
| `dump_dir` | transport default | remote directory crash dumps are read from |

### `[dependencies]`

Prebuilt Windows libraries fetched by `lsw deps add <name>` from the MSYS2
mingw package repos (sha256-verified, cached under `~/.cache/lsw`):

```toml
[dependencies]
zlib = "1.3.1-1"
```

## Environment variables LSW reads

| variable | meaning |
|----------|---------|
| `LSW_WINE` | absolute path to the wine binary to use instead of `wine` from `PATH` (WineHQ `/opt` builds, Proton, Nix) |
| `LSW_TOOLCHAIN_DIRS` | colon-separated directories searched for cross toolchains before `PATH` (e.g. an extracted llvm-mingw `bin/`) |
| `LSW_WINE_X86_64` / `LSW_WINE_X86` / `LSW_WINE_AARCH64` / `LSW_WINE_ARM` | per-architecture Wine builds for qemu user-mode emulation of cross-family targets |
| `LSW_WINRM_PASSWORD` | password for `[verify] transport = "winrm"` |
| `LSW_PFX_B64` / `LSW_PFX_PASSWORD` | signing certificate (base64 PKCS#12) and passphrase for CI signing |
| `NO_COLOR` | disable colored output |

## Environment variables LSW sets for child processes

Build and test commands (and programs run through `lsw run`/`lsw exec`) see:

| variable | meaning |
|----------|---------|
| `LSW_ENV` | name of the active environment |
| `LSW_PROJECT` | absolute host path of the project root |
| `LSW_TARGET_FLAGS` | the cross C flags LSW passed to the toolchain |
| `LSW_HEADLESS` | set to `1` by `lsw test --headless` / `lsw check --headless` |

## User configuration file

`~/.config/lsw/config.toml`:

| key | meaning |
|-----|---------|
| `default_environment` | environment used when a project has no `[environment] name` |
