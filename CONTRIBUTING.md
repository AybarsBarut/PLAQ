# Contributing to PLAQ

PLAQ welcomes reproducible codec experiments, parser hardening, transport tests,
and documentation improvements. Open an issue before changing the on-disk
format. A format proposal must state compatibility consequences and include
round-trip and malformed-input tests.

## Local quality gate (no hosted CI required)

PLAQ intentionally keeps validation local, so contributing does not require a
GitHub Actions billing setup. Run the complete cross-platform gate with:

```bash
python tools/check.py
```

For a faster inner loop, use `python tools/check.py --quick`. To run the full
gate automatically before every push, enable the committed Git hook once per
clone:

```bash
git config core.hooksPath .githooks
```

The hook rejects the push if formatting, Clippy, tests, Python compilation,
documentation, or fuzz-harness compilation fails. These checks run on the local
machine and consume no hosted CI minutes.

Use generated signals or redistributable fixtures only. Benchmark reports must
record the command, platform, signal properties, and baselines. Negative results
are useful and should not be removed merely because PLAQ loses to another codec.

All contributions are accepted under Apache-2.0.
