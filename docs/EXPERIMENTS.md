# Experiments

All committed numbers must come from a recorded command on generated or
redistributable input. This document initially records the repository's
deterministic synthetic smoke corpus; results are filled after the release build
and final verification run.

## Environment

- Date: 2026-09-03
- OS: Windows (local development host)
- Rust: 1.97.1
- Input generator: `tools/generate_synthetic.py`, seed `0x504C4151`
- FLAC: unavailable on the measurement host

## Commands

```bash
python tools/generate_synthetic.py target/experiment.wav --seconds 2
cargo run --release -p plaq-cli -- benchmark target/experiment.wav --compare wav,flac --json
```

## Results

| Signal | PCM bytes | WAV bytes | PLAQ bytes | PLAQ/PCM | FLAC bytes | Encode MiB/s | Decode MiB/s | Bit-perfect |
|---|---:|---:|---:|---:|---:|---:|---:|:---:|
| 2 s synthetic stereo 48 kHz/16-bit | 384000 | 384044 | 244072 | 0.6356 | N/A | 4.63 | 55.88 | yes |

Predictor selections across 24 blocks (48 components): raw 0, delta 21,
linear2 5, cubic3 22, cross-axis 0. The lack of a cross-axis win on this signal
is a result, not an omitted measurement.

## Localhost transport smoke test

The same 244072-byte `.plaq` file was sent through both transports. TCP and UDP
outputs had the same file SHA-256 as the source:
`2ac56cce6728af6fcbea33c3e7910bb0cee079dff60a73a3f5376fe81e1598ce`.

| Transport | Packets | Reordered | Estimated lost | Recovered | Underruns | Measured latency |
|---|---:|---:|---:|---:|---:|---:|
| TCP localhost | N/A | N/A | 0 | N/A | 0 | not instrumented |
| UDP localhost | 209 | 0 | 0 | 0 | 0 | 3.623 ms |

The UDP latency is one local smoke-run measurement from packetization timestamp
to final-block receipt, not a network performance guarantee. Automated tests
also inject reversal, duplication, corruption, and loss; missing data is not
concealed.

The corpus combines correlated 440 Hz tones, diverging chirps, deterministic
low-amplitude white noise, and opposite-polarity impulses. It is a functional
smoke benchmark, not representative music. A positive or negative ratio on this
single signal does not establish general superiority.

## Next experiments

- Compare varied licensed corpora by genre and channel correlation.
- Separate predictor wins by silence, tonal, transient, and noise blocks.
- Measure block-size tradeoffs and a range coder branch against identical
  transforms.
- Install a pinned FLAC version and record its command/version/output without
  equating PLAQ's research goal to guaranteed superiority.
