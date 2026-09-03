# Streaming protocol

PLAQ transport moves already encoded `.plaq` bytes. It cannot change predictors,
reduce bit depth, or switch to a lossy codec when a link is congested.

## TCP demo

The sender listens for one connection and writes:

```text
"PTC1" | file_length:u64le | file_sha256:32 bytes | exact .plaq bytes
```

The receiver limits length to 1 GiB, reads exactly that many bytes, verifies the
transport SHA-256, then validates the entire PLAQ container before writing it.
This is a reliable byte-stream proof, not USB Audio Class support.

## UDP packet version 1

Every datagram contains one 44-byte header and one fragment:

| Offset | Size | Field |
|---:|---:|---|
| 0 | 4 | ASCII `PPK1` |
| 4 | 1 | version = 1 |
| 5 | 1 | flags; bit 0 marks final file block |
| 6 | 2 | header length = 44 |
| 8 | 4 | stream ID |
| 12 | 8 | packet sequence |
| 20 | 8 | sender timestamp, Unix microseconds |
| 28 | 4 | block ID |
| 32 | 2 | fragment index |
| 34 | 2 | fragment count |
| 36 | 2 | payload length |
| 38 | 2 | reserved |
| 40 | 4 | CRC32C of fragment payload |
| 44 | variable | fragment payload |

The demo uses 32 KiB transport blocks and 1200-byte fragment payloads. The
receiver's bounded jitter buffer accepts reordering and duplicates, assembles a
block only when every fragment is present, and releases blocks in ID order. A
timeout, checksum failure, or missing fragment returns an explicit error; it
does not synthesize samples or write a partial `.plaq` file.

Statistics include packets received, reordered, duplicated, estimated missing,
late, checksum failures, recovered, and underruns. `recovered` remains zero in
version 1 because XOR/RS FEC is roadmap work.

## Scope

This is application-level framing over TCP/UDP. Authentication, encryption,
congestion control, retransmission, multicast, USB, Bluetooth, Wi-Fi Direct,
and production clock synchronization are not implemented.

