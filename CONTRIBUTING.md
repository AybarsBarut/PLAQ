# Contributing to PLAQ

PLAQ welcomes reproducible codec experiments, parser hardening, transport tests,
and documentation improvements. Open an issue before changing the on-disk
format. A format proposal must state compatibility consequences and include
round-trip and malformed-input tests.

## Local checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
python -m py_compile tools/*.py
```

Use generated signals or redistributable fixtures only. Benchmark reports must
record the command, platform, signal properties, and baselines. Negative results
are useful and should not be removed merely because PLAQ loses to another codec.

All contributions are accepted under Apache-2.0.

