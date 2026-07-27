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

`--bundle-deps` copies the non-system DLL closure of each artifact into
the package.

## Package metadata

Add a `[package]` section to `lsw.toml`:

```toml
[package]
version       = "1.2.0"
publisher     = "Acme Corp"
description   = "Example application"
icon          = "assets/app.ico"
shortcuts     = true
```

The metadata feeds the MSI, MSIX, NSIS, and winget outputs. LSW also
embeds it into the built PE binary: the icon, a VERSIONINFO resource, and
an application manifest (`dpi_aware`, `requires_admin`). See the
[configuration reference](../reference/configuration.md#package) for all
keys.

With `shortcuts = true`, the MSI and NSIS installers create a Start-menu
shortcut for each executable.

The winget target needs `installer_url`: the URL where you will host the
installer. LSW writes the three manifest files with the installer's
SHA-256 checksum.

## Verify the installer

`lsw package --target msi --verify` tests the MSI before it reports
success:

1. Clone the active environment to a scratch prefix.
2. Install the MSI quietly with `msiexec /i`.
3. Make sure that each packaged file is under Program Files.
4. Uninstall with `msiexec /x`.
5. Make sure that no files remain.

A failure shows as `LSW2040` with the msiexec output.

## Signing

MSIX packages are built natively (manifest, block map, OPC zip) and signed
with a cached self-signed identity.

`lsw sign <pe>` signs one binary:

- Default: the self-signed development identity.
- `--pfx cert.pfx --pfx-pass-env VAR`: a real PKCS#12 certificate. The
  passphrase comes from the environment variable, never from a flag.
- `--timestamp-url <url>`: RFC3161 timestamping, so the signature outlives
  the certificate.

`lsw sign <pe> --verify` checks an existing signature with osslsigncode.

Note: self-signed binaries install only where the certificate is trusted,
or in Windows developer mode.

## Reproducible builds

`lsw build --reproducible` makes byte-identical artifacts. It passes
`-Wl,--no-insert-timestamp` to the linker and sets the PE `TimeDateStamp`
to zero. `lsw verify --reproducible` proves the property: it builds two
times and compares.
