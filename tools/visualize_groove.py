#!/usr/bin/env python3
"""Render signal-domain evidence for the PLAQ trajectory hypothesis."""

from __future__ import annotations

import argparse
import wave
from pathlib import Path

import matplotlib.pyplot as plt
import numpy as np


PREDICTOR_NAMES = ("raw", "delta", "linear2", "cubic3", "cross-axis")


def read_pcm(path: Path) -> tuple[int, np.ndarray]:
    with wave.open(str(path), "rb") as wav:
        channels = wav.getnchannels()
        width = wav.getsampwidth()
        rate = wav.getframerate()
        frames = wav.getnframes()
        if channels not in (1, 2) or width not in (2, 3):
            raise ValueError("only mono/stereo 16-bit or 24-bit PCM WAV is supported")
        raw = wav.readframes(frames)

    if width == 2:
        samples = np.frombuffer(raw, dtype="<i2").astype(np.int32)
    else:
        packed = np.frombuffer(raw, dtype=np.uint8).reshape(-1, 3)
        values = (
            packed[:, 0].astype(np.int32)
            | (packed[:, 1].astype(np.int32) << 8)
            | (packed[:, 2].astype(np.int32) << 16)
        )
        samples = np.where(values & 0x800000, values | ~0xFFFFFF, values).astype(np.int32)
    return rate, samples.reshape(-1, channels)


def trajectory_axes(samples: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    if samples.shape[1] == 1:
        mono = samples[:, 0].astype(np.int64)
        return mono, np.zeros_like(mono)
    left = samples[:, 0].astype(np.int64)
    right = samples[:, 1].astype(np.int64)
    side = left - right
    mid = right + np.floor_divide(side, 2)
    return mid, side


def predictor_residuals(
    values: np.ndarray, predictor: int, reference: np.ndarray | None = None
) -> np.ndarray:
    padded = np.pad(values.astype(np.int64), (3, 0))
    current = padded[3:]
    p1, p2, p3 = padded[2:-1], padded[1:-2], padded[:-3]
    if predictor == 0:
        predicted = np.zeros_like(current)
    elif predictor == 1:
        predicted = p1
    elif predictor == 2:
        predicted = 2 * p1 - p2
    elif predictor == 3:
        predicted = 3 * p1 - 3 * p2 + p3
    elif reference is not None:
        predicted = reference.astype(np.int64)
    else:
        raise ValueError("cross-axis predictor requires a reference")
    return current - predicted


def zigzag(values: np.ndarray) -> np.ndarray:
    return np.where(values >= 0, values * 2, -values * 2 - 1).astype(np.uint64)


def select_predictor(values: np.ndarray, reference: np.ndarray | None = None) -> int:
    best_id, best_bits = 0, None
    predictor_ids = range(5) if reference is not None else range(4)
    for predictor in predictor_ids:
        unsigned = zigzag(predictor_residuals(values, predictor, reference))
        for k in range(32):
            bits = int(np.sum(unsigned >> np.uint64(k))) + len(values) * (1 + k)
            if best_bits is None or bits < best_bits:
                best_id, best_bits = predictor, bits
    return best_id


def render(input_path: Path, output_path: Path, block_frames: int) -> None:
    rate, samples = read_pcm(input_path)
    mid, side = trajectory_axes(samples)
    frames = len(samples)
    time = np.arange(frames) / rate
    stride = max(1, frames // 25_000)

    predictor_rows: list[tuple[int, int, int]] = []
    for block_id, start in enumerate(range(0, frames, block_frames)):
        end = min(frames, start + block_frames)
        predictor_rows.append(
            (
                block_id,
                select_predictor(mid[start:end]),
                select_predictor(side[start:end], mid[start:end]),
            )
        )

    plt.style.use("dark_background")
    figure, axes = plt.subplots(5, 1, figsize=(14, 18), constrained_layout=True)
    figure.suptitle(f"PLAQ virtual stylus analysis — {input_path.name}", fontsize=16)

    left = samples[:, 0]
    right = samples[:, min(1, samples.shape[1] - 1)]
    axes[0].plot(time[::stride], left[::stride], linewidth=0.65, label="left/mono", color="#5eead4")
    axes[0].plot(time[::stride], right[::stride], linewidth=0.55, label="right", color="#fb7185", alpha=0.8)
    axes[0].set(title="PCM waveform", xlabel="seconds", ylabel="sample")
    axes[0].legend(loc="upper right")

    axes[1].plot(time[::stride], mid[::stride], linewidth=0.65, label="mid / lateral", color="#60a5fa")
    axes[1].plot(time[::stride], side[::stride], linewidth=0.55, label="side / vertical", color="#fbbf24", alpha=0.8)
    axes[1].set(title="Reversible virtual axes", xlabel="seconds", ylabel="integer axis")
    axes[1].legend(loc="upper right")

    trajectory_count = min(frames, max(2_000, rate // 5))
    trajectory_stride = max(1, trajectory_count // 8_000)
    axes[2].plot(
        mid[:trajectory_count:trajectory_stride],
        side[:trajectory_count:trajectory_stride],
        linewidth=0.45,
        color="#c084fc",
    )
    axes[2].set(title="Virtual stylus trajectory (opening segment)", xlabel="mid / lateral", ylabel="side / vertical")

    delta = np.diff(mid, prepend=0)
    delta2 = np.diff(delta, prepend=0)
    percentile = max(1, int(np.percentile(np.abs(np.concatenate((delta, delta2))), 99)))
    bins = np.linspace(-percentile, percentile, 151)
    axes[3].hist(delta, bins=bins, alpha=0.65, label="delta", color="#34d399", density=True)
    axes[3].hist(delta2, bins=bins, alpha=0.55, label="delta-delta", color="#f472b6", density=True)
    axes[3].set(title="Residual histograms (central 99%)", xlabel="integer residual", ylabel="density")
    axes[3].legend(loc="upper right")

    block_ids = [row[0] for row in predictor_rows]
    axes[4].scatter(block_ids, [row[1] for row in predictor_rows], s=30, label="mid", color="#60a5fa")
    axes[4].scatter(block_ids, [row[2] + 0.08 for row in predictor_rows], s=24, label="side", color="#fbbf24")
    axes[4].set_yticks(range(5), PREDICTOR_NAMES)
    axes[4].set(title=f"Selected predictor by {block_frames}-frame block", xlabel="block", ylabel="predictor")
    axes[4].legend(loc="upper right")

    for axis in axes:
        axis.grid(alpha=0.18)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    figure.savefig(output_path, dpi=160)
    plt.close(figure)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--block-frames", type=int, default=4096)
    args = parser.parse_args()
    if args.block_frames <= 0:
        parser.error("--block-frames must be positive")
    render(args.input, args.out, args.block_frames)


if __name__ == "__main__":
    main()
