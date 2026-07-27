# CI

## Generate a pipeline

- `lsw ci init github` writes `.github/workflows/lsw.yml`.
- `lsw ci init gitlab` writes `.gitlab-ci.yml`.

Both pipelines do these steps:

1. Install Wine, MinGW, and the build tools.
2. Cache the compiled `lsw` binary, keyed on its version.
3. Cache the environments and the package downloads, keyed on `lsw.lock`.
4. Run `lsw setup`, `lsw build`, and `lsw test --headless`.

The GitHub workflow also has two dispatch-gated jobs: reproducibility
verification (`lsw verify --reproducible`) and artifact signing from a
repository-secret PFX certificate.

## Parts for your own pipeline

- `lsw test --junit report.xml` - a test report for CI ingestion.
- `lsw test --headless` - GUI tests under a virtual display.
- `lsw env export` and `lsw env import-archive` - move a prepared
  environment through the CI cache.
- `lsw env restore <name>` - rebuild from `lsw.lock`, and fail loudly on
  drift.
- `lsw size --baseline <pe> --max-growth <pct>` - a size-regression gate.
- `--format json` on report commands - machine-readable output.
- `lsw verify --native` - the real Windows verdict, when the runner can
  reach a `[verify]` host.
