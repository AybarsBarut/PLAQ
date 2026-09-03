# PLAQ binary format version 1

Status: experimental. Extension: `.plaq`. Multi-byte integers are unsigned
little-endian unless stated otherwise. Signed PCM uses two's-complement values.
Readers must reject unsupported versions and invalid lengths; guessing is not
permitted.

## File header

The fixed header is 64 bytes and is immediately followed by `metadata_length`
bytes. Streaming decoders therefore receive all required audio metadata before
the first block.

| Offset | Size | Field | Version 1 meaning |
|---:|---:|---|---|
| 0 | 4 | magic | ASCII `PLAQ` |
| 4 | 2 | version | `1` |
| 6 | 2 | header length | 64 + metadata length |
| 8 | 1 | profile | `0` = lossless trajectory |
| 9 | 1 | channels | `1` or `2` |
| 10 | 1 | PCM bits | `16` or `24` |
| 11 | 1 | reserved | write zero; ignored in v1 |
| 12 | 4 | sample rate | 1–768000 Hz |
| 16 | 8 | total frames | samples per channel |
| 24 | 4 | block frames | nominal maximum, 1–1048576 |
| 28 | 4 | metadata length | bounded to 1 MiB by reference decoder |
| 32 | 32 | PCM SHA-256 | canonical interleaved PCM bytes |

Canonical PCM bytes are each signed sample truncated to its declared 2 or 3
little-endian bytes in channel-interleaved frame order. The hash excludes RIFF
headers and arbitrary WAV chunks.

Metadata is a sequence of TLVs: `kind:u16`, `length:u32`, then `length` opaque
bytes. Unknown kinds are retained by the API and may be ignored by a v1 decoder.
The field length must fit completely within the declared metadata region.

## Reversible axes

Stereo is converted with mathematical floor division:

```text
side = left - right
mid  = right + floor(side / 2)

right = mid - floor(side / 2)
left  = side + right
```

The components are called `mid/lateral` and `side/vertical`. Mono has one
identity component. Version 1 only accepts 16/24-bit input, so transformed
values fit `i32`.

## Block

Blocks are sequential and independently decodable; every predictor history is
zero at block start.

| Relative offset | Size | Field |
|---:|---:|---|
| 0 | 4 | ASCII `BLK1` |
| 4 | 4 | sequential block id, starting at zero |
| 8 | 4 | frames in this block |
| 12 | 1 | component count; equals channels |
| 13 | 1 | flags; bit 0 must be set for reset state |
| 14 | 2 | descriptor bytes; `component_count * 4` |
| 16 | 4 | compressed payload bytes |
| 20 | 4 | CRC32C of compressed payload only |
| 24 | variable | component descriptors |
| after descriptors | variable | one shared MSB-first Rice bitstream |

Each 4-byte descriptor is `predictor:u8`, `rice_k:u8`, `reserved:u16`.
Predictor identifiers are:

| ID | Name | Estimate for sample n |
|---:|---|---|
| 0 | raw | 0 |
| 1 | delta | x[n-1] |
| 2 | linear2 | 2x[n-1] - x[n-2] |
| 3 | cubic3 | 3x[n-1] - 3x[n-2] + x[n-3] |
| 4 | cross-axis | component 0 at sample n; component 1 only |

Unavailable history is zero. The cross-axis candidate is evaluated only for the
second stereo component because the first has already been decoded. For each
component, signed residuals are ZigZag
mapped: non-negative `2r`, negative `-2r-1`. With Rice parameter `k` (0–31),
write `value >> k` zero bits, one stop bit, then the low `k` bits. Components
are serialized in descriptor order without byte alignment; trailing pad bits in
the block's final byte are zero and semantically ignored.

The encoder evaluates every predictor and every `k` and stores the combination
with the smallest exact bit count. Container overhead is common to every
candidate and does not affect selection.

## Safety and termination

The file has exactly enough sequential blocks to produce `total_frames`, then
EOF. A zero-frame file has no blocks. The reference decoder rejects trailing
bytes, non-sequential IDs, zero/oversized blocks, unknown predictors, `k > 31`,
payloads over 64 MiB, totals over 268435456 samples, CRC mismatches, truncated
fields, arithmetic overflow, and a unary Rice quotient over 1000000. It verifies
the final PCM SHA-256 before returning samples.

## Versioning

A future incompatible layout increments `version`. New metadata kinds require no
version change. New profiles must specify their transforms, integrity rules, and
whether they are lossless; profile 0 must never silently become lossy.
