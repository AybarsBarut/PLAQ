# Benchmarks

The user-facing `plaq benchmark` command is the reproducible benchmark harness.
It measures complete encode/decode operations, exact sizes, predictor counts, and
bit-perfect status. Raw results and environment notes belong in
`docs/EXPERIMENTS.md`; microbenchmarks may be added here when a specific hot path
needs isolation.

