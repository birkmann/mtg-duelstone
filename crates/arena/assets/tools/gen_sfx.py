#!/usr/bin/env python3
"""Synthesise the game's sound effects + ambient loop (numpy → wav → ffmpeg → ogg vorbis)."""
import os, subprocess, tempfile, wave
import numpy as np

SR = 44100
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "sfx")
os.makedirs(OUT, exist_ok=True)
rng = np.random.default_rng(3)


def t(d): return np.arange(int(d * SR)) / SR
def env(n, a=0.005, r=0.1, curve=2.0):
    e = np.ones(n); na = max(1, int(a * SR)); nr = max(1, int(r * SR))
    e[:na] = np.linspace(0, 1, na); e[-nr:] *= np.linspace(1, 0, nr) ** curve
    return e
def expdecay(d, k): return np.exp(-t(d) * k)
def noise(d): return rng.standard_normal(int(d * SR))
def fft_filter(x, lo=None, hi=None, order=2):
    X = np.fft.rfft(x); f = np.fft.rfftfreq(len(x), 1 / SR); g = np.ones_like(f)
    if lo: g *= 1 / (1 + (lo / np.maximum(f, 1e-3)) ** (2 * order))
    if hi: g *= 1 / (1 + (f / hi) ** (2 * order))
    return np.fft.irfft(X * g, len(x))
def bandnoise(d, lo, hi): return fft_filter(noise(d), lo, hi)
def sine(f, d, ph=0): return np.sin(2 * np.pi * f * t(d) + ph)
def sweep(f0, f1, d, exp=True):
    tt = t(d)
    f = f0 * (f1 / f0) ** (tt / d) if exp else f0 + (f1 - f0) * tt / d
    return np.sin(2 * np.pi * np.cumsum(f) / SR)
def mix(*parts, n=None):
    n = n or max(len(p[0]) + int(p[1] * SR) for p in parts)
    o = np.zeros(n)
    for s, off, *g in parts:
        a = int(off * SR); gain = g[0] if g else 1
        e = min(n, a + len(s)); o[a:e] += s[: e - a] * gain
    return o
def reverb(x, wet=0.25, tail=0.35):
    n = int(tail * SR); ir = noise(tail) * np.exp(-t(tail) * 9); ir = fft_filter(ir, 300, 6000)
    y = np.convolve(np.pad(x, (0, n)), ir / np.abs(ir).sum() * 6, mode="full")[: len(x) + n]
    return np.pad(x, (0, n)) + wet * y
def bell(f, d, k=5, h=(1, 2.01, 2.76, 4.2), g=(1, .5, .3, .15)):
    return sum(gi * sine(f * hi, d) for hi, gi in zip(h, g)) * expdecay(d, k)
def finish(x, peak=0.85):
    x = np.tanh(x * 1.1); x = x / max(1e-6, np.abs(x).max()) * peak
    n = len(x); x[-64:] *= np.linspace(1, 0, 64); x[:16] *= np.linspace(0, 1, 16); return x


def save(name, x, music=False):
    x = np.asarray(x, np.float32)
    tmp = tempfile.mktemp(suffix=".wav")
    with wave.open(tmp, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype("<i2").tobytes())
    q = "3" if music else "5"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", tmp, "-c:a", "libvorbis", "-q:a", q, os.path.join(OUT, name + ".ogg")], check=True)
    os.remove(tmp); print("wrote", name)


# ---- one-shots
def card_draw():
    d = 0.22; n = bandnoise(d, 1800, 9000) * env(int(d * SR), .002, .12) * (0.5 + 0.5 * np.linspace(0, 1, int(d * SR)))
    sl = bandnoise(d, 500, 3000) * expdecay(d, 14) * 0.5
    return finish(n + sl, .7)
def card_play():
    d = 0.35; thump = sweep(180, 60, d) * expdecay(d, 14) * .9
    slap = bandnoise(d, 800, 6000) * expdecay(d, 40) * .8
    return finish(reverb(thump + slap, .15, .25), .8)
def land_drop():
    d = 0.5; thump = sweep(120, 45, d) * expdecay(d, 9) * 1.0
    grit = bandnoise(d, 100, 1500) * expdecay(d, 16) * .6
    tick = bandnoise(.05, 2000, 8000) * expdecay(.05, 70)
    return finish(reverb(mix((thump, 0), (grit, 0), (tick, 0), n=int(d * SR)), .2, .3), .85)
def spell_cast():
    d = 1.1; tt = t(d)
    rise = sweep(220, 1760, d) * env(len(tt), .05, .4) * .4
    shimmer = sum(sine(f * (1 + .003 * i), d) for i, f in enumerate([880, 1320, 1760, 2640])) * env(len(tt), .3, .6) * .12
    air = bandnoise(d, 2000, 12000) * env(len(tt), .5, .5) * .25
    sparkle = sum(bell(f, .4, 12, h=(1,), g=(1,)) * 0.3 for f in [])  # none
    pop = bell(1320, .6, 6) * .3
    return finish(reverb(mix((rise, 0), (shimmer, 0), (air, 0), (pop, .55), n=int(1.4 * SR)), .35, .5), .85)
def resolve():
    d = .6
    a = bell(660, d, 6) * .5; b = bell(990, d, 7) * .35; c = bell(1320, d, 8, ) * .25
    return finish(reverb(mix((a, 0), (b, .06), (c, .12)), .3, .4), .75)
def attack_whoosh():
    d = .6; n = fft_filter(noise(d), 300, 5000)
    sweepf = np.linspace(0, 1, int(d * SR))
    # moving band via amplitude of two filtered noises crossfaded
    lo = bandnoise(d, 200, 1200); hi = bandnoise(d, 1500, 8000)
    e = np.sin(np.pi * sweepf) ** 2
    x = (lo * (1 - sweepf) + hi * sweepf) * e
    thud = sweep(110, 50, .3) * expdecay(.3, 12)
    return finish(mix((x * .9, 0), (thud * .6, .3), n=int(d * SR + .1 * SR)), .85)
def hit_impact():
    d = .45; body = sweep(200, 45, d) * expdecay(d, 11) * 1.0
    crack = bandnoise(d, 1200, 9000) * expdecay(d, 38) * .9
    mid = bandnoise(d, 300, 2500) * expdecay(d, 20) * .6
    return finish(reverb(body + crack + mid, .15, .2), .95)
def life_loss():
    d = .7; a = sweep(440, 110, d) * env(int(d * SR), .01, .5) * .6
    b = sweep(445, 108, d) * env(int(d * SR), .01, .5) * .4
    thump = sweep(100, 40, .3) * expdecay(.3, 10)
    return finish(mix((a + b, 0), (thump * .7, 0), n=int(d * SR)), .8)
def life_gain():
    notes = [523.25, 659.25, 783.99, 1046.5]
    parts = [(bell(f, .7, 5, h=(1, 2, 3), g=(1, .25, .1)) * .5, i * .07) for i, f in enumerate(notes)]
    return finish(reverb(mix(*parts), .35, .5), .75)
def turn_start():
    a = bell(392, 1.2, 3.2, h=(1, 2.0, 3.0, 4.07), g=(1, .5, .25, .12))
    b = bell(587.3, 1.0, 3.8, h=(1, 2.0, 3.0), g=(1, .4, .15))
    return finish(reverb(mix((a * .6, 0), (b * .5, .14)), .4, .6), .8)
def button_click():
    d = .09; x = sweep(900, 500, d) * expdecay(d, 50) * .6 + bandnoise(d, 2500, 9000) * expdecay(d, 120) * .4
    return finish(x, .7)
def hover_tick():
    d = .05; return finish(sweep(1800, 1500, d) * expdecay(d, 80) * .5 + bandnoise(d, 4000, 12000) * expdecay(d, 150) * .15, .35)
def victory():
    notes = [(392, 0), (523.25, .14), (659.25, .28), (783.99, .42), (1046.5, .6)]
    parts = [(bell(f, 1.6, 2.4, h=(1, 2, 3, 4), g=(1, .4, .2, .1)) * .45, o) for f, o in notes]
    chord = [(bell(f, 2.2, 1.8, h=(1, 2, 3), g=(1, .3, .12)) * .35, .75) for f in (523.25, 659.25, 783.99, 1046.5)]
    return finish(reverb(mix(*parts, *chord), .45, .9), .85)
def defeat():
    notes = [(392, 0), (349.2, .3), (311.1, .6), (261.6, .95)]
    parts = [(fft_filter(sine(f, 1.4) * .5 + sine(f * 2, 1.4) * .18 + sine(f * 1.5, 1.4) * .1, None, 1800) * env(int(1.4 * SR), .03, .9) * .8, o) for f, o in notes]
    low = sine(65.4, 2.2) * env(int(2.2 * SR), .1, 1.2) * .5
    return finish(reverb(mix(*parts, (low, .95)), .4, .8), .85)


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
    for nm, fn in [("card_draw", card_draw), ("card_play", card_play), ("land_drop", land_drop), ("spell_cast", spell_cast),
                   ("resolve", resolve), ("attack_whoosh", attack_whoosh), ("hit_impact", hit_impact), ("life_loss", life_loss),
                   ("life_gain", life_gain), ("turn_start", turn_start), ("button_click", button_click), ("hover_tick", hover_tick),
                   ("victory", victory), ("defeat", defeat)]:
        save(nm, fn())
    save("ambient", ambient(), music=True)
    save("ambient_match", ambient(True), music=True)
