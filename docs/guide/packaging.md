# Packaging and signing

`lsw package` turns the build output into a distributable:

```
lsw package --target portable-directory   # dist/<name>-<arch>/
lsw package --target zip                  # + .zip
lsw package --target msi                  # Windows Installer (needs wixl/msitools)
lsw package --target msix                 # signed MSIX (needs zip, osslsigncode, openssl)
lsw package --target nsis                 # NSIS setup.exe (needs makensis)
lsw package --target winget               # MSI + winget manifests
```

A `[package]` section in `lsw.toml` (version, publisher, description, icon,
shortcuts) feeds the installer metadata and is embedded into the built PE
as an icon, VERSIONINFO resource, and application manifest - see the
[configuration reference](../reference/configuration.md).

`lsw package --target msi --verify` installs the MSI quietly into a scratch
clone of the environment, checks every file landed under Program Files,
uninstalls, and verifies nothing was left behind (failures show as
`LSW2040`).

## Signing

MSIX packages are built natively (manifest, block map, OPC zip) and signed
with a cached self-signed identity; `lsw sign <pe>` does the same for one
binary, or signs with a real PFX certificate
(`--pfx cert.pfx --pfx-pass-env VAR`, `--timestamp-url` for RFC3161).
`lsw sign <pe> --verify` checks an existing signature. Self-signed
artifacts install only where the certificate is trusted, or in Windows
developer mode.

## Reproducible builds

`lsw build --reproducible` makes byte-identical artifacts: it passes
`-Wl,--no-insert-timestamp` to the linker and zeroes the PE `TimeDateStamp`
in every output. `lsw verify --reproducible` proves it by building twice
and diffing.
