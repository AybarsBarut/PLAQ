#!/usr/bin/env python3
"""Generate deterministic, license-free PCM WAV signals for PLAQ experiments."""

from __future__ import annotations

import argparse
import math
import random
import struct
import wave
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seconds", type=float, default=2.0)
    parser.add_argument("--sample-rate", type=int, default=48_000)
    parser.add_argument("--bits", type=int, choices=(16, 24), default=16)
    parser.add_argument("--channels", type=int, choices=(1, 2), default=2)
    args = parser.parse_args()
    if args.seconds <= 0 or args.sample_rate <= 0:
        parser.error("duration and sample rate must be positive")

    randomizer = random.Random(0x504C4151)
    frames = int(args.seconds * args.sample_rate)
    peak = (1 << (args.bits - 1)) - 1
    samples: list[int] = []
    for index in range(frames):
        t = index / args.sample_rate
        sweep_phase = 2 * math.pi * (90 * t + 750 * t * t)
        impulse = 0.45 if index % (args.sample_rate // 4) == 0 else 0.0
        noise = randomizer.uniform(-0.025, 0.025)
        left = 0.46 * math.sin(2 * math.pi * 440 * t) + 0.16 * math.sin(sweep_phase) + impulse + noise
        right = 0.43 * math.sin(2 * math.pi * 440 * t + 0.15) + 0.13 * math.sin(sweep_phase * 1.003) - impulse + noise
        samples.append(max(-peak - 1, min(peak, round(left * peak))))
        if args.channels == 2:
            samples.append(max(-peak - 1, min(peak, round(right * peak))))

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(args.output), "wb") as wav:
        wav.setnchannels(args.channels)
        wav.setsampwidth(args.bits // 8)
        wav.setframerate(args.sample_rate)
        if args.bits == 16:
            wav.writeframes(b"".join(struct.pack("<h", value) for value in samples))
        else:
            wav.writeframes(b"".join((value & 0xFFFFFF).to_bytes(3, "little") for value in samples))


if __name__ == "__main__":
    main()

