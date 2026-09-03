## Summary

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] Lossless changes include a bit-perfect round-trip test
- [ ] Format changes update `docs/FORMAT.md` and compatibility notes
- [ ] Benchmark claims include commands, input properties, and raw results

## Scientific integrity

- [ ] No unsupported quality or compression claim is introduced
- [ ] Lossy simulation remains separated from the lossless profile

