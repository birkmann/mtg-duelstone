#!/usr/bin/env python3
"""Synthesise the two ambient music loops (numpy → wav → ffmpeg → ogg vorbis).

The one-shot sound effects used to be synthesised here too; they are now built from CC0 recordings
by build_sfx.py.
"""
import os, subprocess, tempfile, wave
import numpy as np

SR = 44100
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "sfx")
os.makedirs(OUT, exist_ok=True)
rng = np.random.default_rng(3)


def t(d): return np.arange(int(d * SR)) / SR
def expdecay(d, k): return np.exp(-t(d) * k)
def fft_filter(x, lo=None, hi=None, order=2):
    X = np.fft.rfft(x); f = np.fft.rfftfreq(len(x), 1 / SR); g = np.ones_like(f)
    if lo: g *= 1 / (1 + (lo / np.maximum(f, 1e-3)) ** (2 * order))
    if hi: g *= 1 / (1 + (f / hi) ** (2 * order))
    return np.fft.irfft(X * g, len(x))
def sine(f, d, ph=0): return np.sin(2 * np.pi * f * t(d) + ph)
def bell(f, d, k=5, h=(1, 2.01, 2.76, 4.2), g=(1, .5, .3, .15)):
    return sum(gi * sine(f * hi, d) for hi, gi in zip(h, g)) * expdecay(d, k)
def save(name, x, music=False):
    x = np.asarray(x, np.float32)
    tmp = tempfile.mktemp(suffix=".wav")
    with wave.open(tmp, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype("<i2").tobytes())
    q = "3" if music else "5"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", tmp, "-c:a", "libvorbis", "-q:a", q, os.path.join(OUT, name + ".ogg")], check=True)
    os.remove(tmp); print("wrote", name)


# ---- ambient: loopable pad, 64 s, crossfaded ends so it loops seamlessly
def ambient(match=False):
    L = 64.0; n = int(L * SR); tt = np.arange(n) / SR
    if match:  # darker, minor, slower motion: sits under gameplay
        chords = [(73.4, 110, 146.8, 174.6), (65.4, 98, 130.8, 155.6), (82.4, 123.5, 164.8, 196), (65.4, 98, 130.8, 164.8)]
    else:
        chords = [(110, 164.8, 220, 277.2), (98, 146.8, 196, 246.9), (87.3, 130.8, 174.6, 220), (98, 146.8, 196, 293.7)]
    out = np.zeros(n); seg = L / len(chords)
    for ci, ch in enumerate(chords):
        a = int(ci * seg * SR); b = a + int((seg + 4) * SR)
        for f in ch:
            for det in (-0.4, 0, 0.5):
                seg_t = np.arange(a, b) / SR
                lfo = 1 + 0.002 * np.sin(2 * np.pi * (0.1 + 0.03 * ci) * seg_t + f)
                w = np.sin(2 * np.pi * f * (1 + det * 0.002) * seg_t * lfo) + 0.3 * np.sin(2 * np.pi * 2 * f * seg_t) * 0.5
                m = len(w); e = np.minimum(1, np.minimum(np.arange(m) / (3 * SR), (m - np.arange(m)) / (3 * SR)))
                idx = np.arange(a, b) % n
                np.add.at(out, idx, w * e * 0.05)
    # slow-moving airy noise
    air = fft_filter(rng.standard_normal(n), 200, 1800)
    out += air * (0.025 + 0.015 * np.sin(2 * np.pi * tt / 16))
    # sparse bell plucks
    for i in range(14):
        f = float(rng.choice([440, 523.25, 587.33, 659.25, 783.99, 880]))
        o = rng.uniform(0, L - 4); bl = bell(f, 3.5, 1.5, h=(1, 2.0), g=(1, .15)) * .05
        a = int(o * SR); out[a:a + len(bl)] += bl[: n - a]
    # stereo-less; ensure loop seam by wrapping tail crossfade
    x = out / np.abs(out).max() * .6
    return x


if __name__ == "__main__":
    save("ambient", ambient(), music=True)
    save("ambient_match", ambient(True), music=True)
