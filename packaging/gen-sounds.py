#!/usr/bin/env python3
"""Generate the typing sound sets from scratch (no recordings, nothing to license).

Writes assets/sounds/gentle/key1-6.wav and assets/sounds/mechanical/key1-6.wav:
44.1 kHz mono 16-bit, peak-normalised to -14 dBFS. Deterministic per seed.
"""
import math, os, random, struct, wave

SR = 44100
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sounds")


def lowpass(samples, cutoff):
    a = 1.0 - math.exp(-2 * math.pi * cutoff / SR)
    y = 0.0
    out = []
    for x in samples:
        y += a * (x - y)
        out.append(y)
    return out


def highpass(samples, cutoff):
    low = lowpass(samples, cutoff)
    return [x - l for x, l in zip(samples, low)]


def env(n, attack_s, tau_s, delay_s=0.0):
    """Exponential decay after a short linear attack, starting at delay."""
    out = []
    att = max(1, int(attack_s * SR))
    d0 = int(delay_s * SR)
    for i in range(n):
        t = i - d0
        if t < 0:
            out.append(0.0)
        elif t < att:
            out.append(t / att)
        else:
            out.append(math.exp(-(t - att) / (tau_s * SR)))
    return out


def tone(n, f0, f1=None, tau_s=0.03, delay_s=0.0):
    """Decaying sine, optionally gliding from f0 to f1 over the first 10 ms."""
    f1 = f0 if f1 is None else f1
    e = env(n, 0.0005, tau_s, delay_s)
    phase = 0.0
    out = []
    glide = int(0.01 * SR)
    for i in range(n):
        k = min(1.0, i / glide) if glide else 1.0
        f = f0 + (f1 - f0) * k
        phase += 2 * math.pi * f / SR
        out.append(math.sin(phase) * e[i])
    return out


def mix(*layers):
    n = max(len(l) for l, _ in layers)
    out = [0.0] * n
    for samples, gain in layers:
        for i, s in enumerate(samples):
            out[i] += s * gain
    return out


def finish(samples, fade_s=0.02, peak_db=-14.0):
    n = len(samples)
    fade = int(fade_s * SR)
    for i in range(n - fade, n):
        samples[i] *= (n - i) / fade
    peak = max(abs(s) for s in samples) or 1.0
    g = 10 ** (peak_db / 20) / peak
    return [max(-1.0, min(1.0, s * g)) for s in samples]


def gentle(rng):
    """A soft, muted thock: low body with a quick pitch drop and a dull puff of air."""
    n = int(0.12 * SR)
    noise = [rng.uniform(-1, 1) for _ in range(n)]
    puff = [x * e for x, e in zip(lowpass(noise, rng.uniform(1400, 2200)), env(n, 0.001, rng.uniform(0.007, 0.011)))]
    f0 = rng.uniform(140, 220)
    body = tone(n, f0 * 1.6, f0, tau_s=rng.uniform(0.022, 0.034))
    tick = tone(n, rng.uniform(3000, 4500), tau_s=0.003)
    return finish(mix((body, 0.8), (puff, 0.55), (tick, 0.12)))


def mechanical(rng):
    """A crisp switch click with a spring ring, a body thump and a bottom-out tap."""
    n = int(0.10 * SR)
    noise = [rng.uniform(-1, 1) for _ in range(n)]
    click = [x * e for x, e in zip(lowpass(highpass(noise, 2000), rng.uniform(5000, 7000)), env(n, 0.0003, rng.uniform(0.003, 0.005)))]
    f_ring = rng.uniform(3200, 4200)
    ring = mix((tone(n, f_ring, tau_s=rng.uniform(0.009, 0.014)), 1.0), (tone(n, f_ring * 1.6, tau_s=0.006), 0.4))
    thump = tone(n, rng.uniform(160, 210) * 1.4, rng.uniform(160, 210), tau_s=0.018)
    body = [x * e for x, e in zip(lowpass(noise, 600), env(n, 0.001, 0.015))]
    bottom_delay = rng.uniform(0.006, 0.010)
    bottom = [x * e for x, e in zip(lowpass(highpass(noise, 1500), 5000), env(n, 0.0003, 0.004, bottom_delay))]
    return finish(mix((click, 1.0), (ring, 0.35), (thump, 0.4), (body, 0.45), (bottom, 0.5)))


def write(path, samples):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    w = wave.open(path, "wb")
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes(struct.pack("<%dh" % len(samples), *(int(s * 32767) for s in samples)))
    w.close()


for name, make in (("gentle", gentle), ("mechanical", mechanical)):
    for i in range(1, 7):
        write(os.path.join(OUT, name, f"key{i}.wav"), make(random.Random(f"{name}-{i}")))
        print(f"{name}/key{i}.wav")
