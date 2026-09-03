# Repository-level tests

Executable Rust integration tests live beside their owning crates so Cargo runs
them automatically. `tests/fixtures/` is reserved for tiny redistributable
fixtures; current tests generate all PCM in memory and commit no audio files.

Cross-crate CLI round-trip coverage is in
`crates/plaq-cli/tests/cli_roundtrip.rs`; malformed container and signal-matrix
coverage is in `crates/plaq-format/tests/`; transport disorder and loss coverage
is in `crates/plaq-stream/tests/`.

