# CI

`lsw ci init github` writes `.github/workflows/lsw.yml`;
`lsw ci init gitlab` writes `.gitlab-ci.yml`. Both pipelines:

- install Wine, MinGW, and the build tools,
- cache the compiled `lsw` binary keyed on its version,
- cache environments and package downloads keyed on `lsw.lock`,
- run `lsw setup && lsw build && lsw test --headless`.

The GitHub workflow also carries dispatch-gated jobs for reproducibility
verification (`lsw verify --reproducible`) and artifact signing from a
repository-secret PFX.

Useful pieces for hand-rolled pipelines:

- `lsw test --junit report.xml` for test-report ingestion.
- `lsw env export` / `lsw env import-archive` to move a prepared
  environment through your CI cache instead of re-running `wineboot`.
- `lsw env restore <name>` to rebuild from `lsw.lock` and fail loudly if
  anything drifted from the pins.
- `--format json` on report commands for machine consumption.
