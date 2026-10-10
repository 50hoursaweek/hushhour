#!/usr/bin/env python3
"""Turn a generated track into a seamless loop.

Trims the intro and the ending, keeps a whole number of bars (tempo found by
autocorrelation of the onset envelope), and cross-fades the last bar into the
first so the app's loop point cannot be heard. Needs ffmpeg on PATH.

    python3 packaging/loopify.py IN.mp3 OUT.ogg [--skip-start=8] [--skip-end=10]
"""
import math, os, struct, subprocess, sys

SR = 44100


def decode(path):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-f", "f32le", "-ac", "2", "-ar", str(SR), "-"], capture_output=True, check=True).stdout
    n = len(raw) // 4
    x = struct.unpack(f"<{n}f", raw)
    return [x[0::2], x[1::2]]


def encode(path, chans):
    n = len(chans[0])
    inter = [0.0] * (2 * n)
    inter[0::2] = chans[0]
    inter[1::2] = chans[1]
    raw = struct.pack(f"<{2 * n}f", *inter)
    codec = ["-c:a", "libvorbis", "-q:a", "6"] if path.endswith(".ogg") else ["-c:a", "flac"]
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "f32le", "-ac", "2", "-ar", str(SR), "-i", "-", *codec, path], input=raw, check=True)


def bar_length(mono, lo_bpm=50, hi_bpm=110):
    """Seconds per bar (4 beats), from the onset envelope's autocorrelation."""
    hop = SR // 100  # 10 ms frames
    energy = [sum(v * v for v in mono[i:i + hop]) for i in range(0, len(mono) - hop, hop)]
    onset = [max(0.0, energy[i] - energy[i - 1]) for i in range(1, len(energy))]
    mean = sum(onset) / len(onset)
    onset = [v - mean for v in onset]
    best, best_lag = -1.0, 0
    for lag in range(int(100 * 60 / hi_bpm), int(100 * 60 / lo_bpm)):
        c = sum(onset[i] * onset[i - lag] for i in range(lag, len(onset)))
        if c > best:
            best, best_lag = c, lag
    beat = best_lag / 100.0
    return beat * 4, 60.0 / beat


def main(argv):
    src, dst = argv[0], argv[1]
    skip_start = float(next((a.split("=")[1] for a in argv if a.startswith("--skip-start=")), 8))
    skip_end = float(next((a.split("=")[1] for a in argv if a.startswith("--skip-end=")), 10))
    chans = decode(src)
    mono = [(l + r) / 2 for l, r in zip(*chans)]
    bar, bpm = bar_length(mono)
    usable = len(mono) / SR - skip_start - skip_end
    bars = int(usable // bar)
    if bars < 4:
        sys.exit("track too short to loop")
    loop_s = bars * bar
    start = int(skip_start * SR)
    fade = int(bar * SR)  # cross-fade over one bar
    total = int(loop_s * SR)
    out = []
    for ch in chans:
        seg = ch[start:start + total + fade]  # one extra bar for the cross-fade
        if len(seg) < total + fade:
            sys.exit("not enough audio after the loop end for the cross-fade")
        body = list(seg[fade:total])
        # The tail (bar after the loop end) blends into the head (first bar).
        tail = []
        for i in range(fade):
            t = i / fade
            a, b = math.cos(t * math.pi / 2), math.sin(t * math.pi / 2)
            tail.append(seg[total + i] * a + seg[i] * b)
        out.append(body + tail)
    encode(dst, out)
    print(f"{os.path.basename(dst)}: {bpm:.1f} bpm, bar {bar:.2f} s, {bars} bars = {loop_s:.1f} s, cross-fade {bar:.2f} s")


if __name__ == "__main__":
    main(sys.argv[1:])
