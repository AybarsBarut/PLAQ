#!/usr/bin/env python3
"""Convert one or more `plaq benchmark --json` reports to Markdown."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reports", nargs="+", type=Path)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()

    rows = []
    for path in args.reports:
        report = json.loads(path.read_text(encoding="utf-8"))
        flac = report.get("flac_bytes")
        rows.append(
            "| {input} | {sample_rate} | {channels} | {bits_per_sample} | {pcm_bytes} | "
            "{plaq_bytes} | {ratio:.4f} | {flac} | {verified} |".format(
                input=Path(report["input"]).name,
                sample_rate=report["sample_rate"],
                channels=report["channels"],
                bits_per_sample=report["bits_per_sample"],
                pcm_bytes=report["pcm_bytes"],
                plaq_bytes=report["plaq_bytes"],
                ratio=report["plaq_to_pcm_ratio"],
                flac=flac if flac is not None else "N/A",
                verified="yes" if report["bit_perfect"] else "NO",
            )
        )
    markdown = "\n".join(
        [
            "| Input | Hz | Channels | Bits | PCM bytes | PLAQ bytes | PLAQ/PCM | FLAC bytes | Bit-perfect |",
            "|---|---:|---:|---:|---:|---:|---:|---:|:---:|",
            *rows,
            "",
        ]
    )
    if args.out:
        args.out.write_text(markdown, encoding="utf-8")
    else:
        print(markdown, end="")


if __name__ == "__main__":
    main()

