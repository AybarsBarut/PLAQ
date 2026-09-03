# Changelog

All notable changes follow Keep a Changelog conventions. PLAQ format
compatibility is documented separately in `docs/FORMAT.md`.

## [Unreleased]

### Added

- Lossless trajectory profile for mono/stereo 16-bit and 24-bit PCM WAV.
- Version 1 `.plaq` container with block CRC32C and whole-PCM SHA-256.
- Raw, delta, linear second-order, and cubic third-order predictors with exact
  per-block Rice parameter selection.
- TCP byte-stream and UDP packet/reassembly demonstrations.
- Separate lossy physical-stylus simulation and diagnostic visualization.
- Property, corruption, truncation, CLI, and packet reordering tests.
- Cross-platform local quality gate and optional pre-push hook, requiring no
  hosted CI or billing configuration.
