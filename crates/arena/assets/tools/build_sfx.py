#!/usr/bin/env python3
"""Build the game's one-shot sound effects (../sfx/<name>_<n>.ogg) from CC0 recordings.

Why recordings: the first set was synthesised from sine sweeps and filtered noise, which reads as
"chiptune" next to a 3D table. Card games live on foley (paper, felt, wood), so physical sounds come
from real recordings and only the musical moments get light synthesis (a chord swell under a sting).

Why several takes per sound: one identical sample fired dozens of times a game is what makes game
audio sound cheap. Every sound has 2-6 takes; the game picks one at random and nudges its pitch and
level (see `audio.rs`).

Sources, all CC0 (public domain), downloaded once into target/sfx-sources (never at runtime; the
output .ogg files are committed): Kenney's Casino Audio, Interface Sounds, Impact Sounds, RPG Audio,
UI Audio and Music Jingles (kenney.nl), and "80 CC0 RPG SFX" by rubberduck (opengameart.org).

Every take is trimmed to its onset (the recordings start with up to 0.5 s of silence, which would
make a card sound lag its animation), layered, given a tail fade, loudness-matched to its sound's
target and peak-limited. Needs numpy and ffmpeg.

    python3 crates/arena/assets/tools/build_sfx.py
"""
import io, os, subprocess, urllib.request, zipfile
import numpy as np

SR = 44100
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, "..", "sfx"))
CACHE = os.path.normpath(os.path.join(HERE, "..", "..", "..", "..", "target", "sfx-sources"))
rng = np.random.default_rng(7)

PACKS = {
    "casino": "https://kenney.nl/media/pages/assets/casino-audio/2472606a04-1721639069/kenney_casino-audio.zip",
    "interface": "https://kenney.nl/media/pages/assets/interface-sounds/fa43c1dd4d-1677589452/kenney_interface-sounds.zip",
    "impact": "https://kenney.nl/media/pages/assets/impact-sounds/87b4ddecda-1677589768/kenney_impact-sounds.zip",
    "rpg": "https://kenney.nl/media/pages/assets/rpg-audio/8e99002d76-1677590336/kenney_rpg-audio.zip",
    "ui": "https://kenney.nl/media/pages/assets/ui-audio/490d233f68-1677590494/kenney_ui-audio.zip",
    "jingles": "https://kenney.nl/media/pages/assets/music-jingles/f37e530b9e-1677590399/kenney_music-jingles.zip",
    "rpg80": "https://opengameart.org/sites/default/files/80-CC0-RPG-SFX_0.zip",
}


# ---- sources --------------------------------------------------------------------------------

def fetch():
    for name, url in PACKS.items():
        d = os.path.join(CACHE, name)
        if os.path.isdir(d):
            continue
        print("downloading", name)
        req = urllib.request.Request(url, headers={"User-Agent": "mtg-duelstone build_sfx"})
        with urllib.request.urlopen(req) as r:
            zipfile.ZipFile(io.BytesIO(r.read())).extractall(d)


_index = None
def src(pack, filename):
    """Path of `filename` anywhere inside a pack (the zips nest files in varying folders)."""
    global _index
    if _index is None:
        _index = {}
        for root, _, files in os.walk(CACHE):
            for f in files:
                rel = os.path.relpath(root, CACHE).split(os.sep)[0]
                _index[(rel, f)] = os.path.join(root, f)
    return _index[(pack, filename)]


def load(pack, filename):
    raw = subprocess.run(["ffmpeg", "-loglevel", "error", "-i", src(pack, filename), "-ac", "1", "-ar", str(SR), "-f", "f32le", "-"],
                         capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.float32).astype(np.float64)


# ---- dsp ------------------------------------------------------------------------------------

def db(x): return 10 ** (x / 20)


def envelope(x, win=0.005):
    n = max(1, int(win * SR))
    return np.convolve(np.abs(x), np.ones(n) / n, mode="same")


def trim(x, pre=0.003, floor_db=-36):
    """Cut leading silence up to `pre` s before the onset, and the inaudible tail."""
    e = envelope(x)
    loud = np.where(e > e.max() * db(floor_db))[0]
    if len(loud) == 0:
        return x
    a = max(0, loud[0] - int(pre * SR))
    tail = np.where(e > e.max() * db(-60))[0]
    b = min(len(x), tail[-1] + int(0.02 * SR))
    return x[a:b]


def fade(x, fin=0.002, fout=0.04):
    x = x.copy()
    a, b = min(len(x), int(fin * SR)), min(len(x), int(fout * SR))
    if a: x[:a] *= np.linspace(0, 1, a)
    if b: x[-b:] *= np.linspace(1, 0, b) ** 2
    return x


def filt(x, lo=None, hi=None, order=2):
    X = np.fft.rfft(x); f = np.fft.rfftfreq(len(x), 1 / SR); g = np.ones_like(f)
    if lo: g *= 1 / (1 + (lo / np.maximum(f, 1e-3)) ** (2 * order))
    if hi: g *= 1 / (1 + (f / hi) ** (2 * order))
    return np.fft.irfft(X * g, len(x))


def pitch(x, semitones):
    """Resample: higher and shorter (tape-style), which keeps transients natural."""
    if semitones == 0:
        return x
    r = 2 ** (semitones / 12)
    idx = np.arange(0, len(x) - 1, r)
    return np.interp(idx, np.arange(len(x)), x)


def layer(*parts):
    """parts: (signal, gain_db, offset_s). Mono sum."""
    n = max(int(o * SR) + len(s) for s, _, o in parts)
    out = np.zeros(n)
    for s, g, o in parts:
        a = int(o * SR)
        out[a:a + len(s)] += s * db(g)
    return out


def reverb(x, wet=0.25, tail=1.2, damp=6000, predelay=0.012):
    """Stereo room from decaying noise (two decorrelated IRs). Returns (n, 2)."""
    n = int(tail * SR); t = np.arange(n) / SR
    out = []
    for _ in range(2):
        ir = rng.standard_normal(n) * np.exp(-t * (6.9 / tail))
        ir = filt(ir, 200, damp)
        ir[: int(predelay * SR)] = 0
        ir /= np.sqrt((ir ** 2).sum())
        y = np.convolve(np.pad(x, (0, n)), ir)[: len(x) + n]
        out.append(np.pad(x, (0, n)) + wet * y * 0.35)
    return np.stack(out, axis=1)


def stereo(x):
    return x if x.ndim == 2 else np.stack([x, x], axis=1)


def loudness(x):
    """Level of the loudest 100 ms (dBFS RMS): what the ear compares between short sounds."""
    m = x.mean(axis=1) if x.ndim == 2 else x
    w = int(0.1 * SR)
    if len(m) <= w:
        return 20 * np.log10(np.sqrt((m ** 2).mean()) + 1e-12)
    p = np.convolve(m ** 2, np.ones(w) / w, mode="valid")
    return 10 * np.log10(p.max() + 1e-12)


# Vorbis overshoots sharp, bright transients (chimes) by up to ~2.5 dB, so leave headroom.
def master(x, target_db, ceiling_db=-3.5):
    x = x * db(target_db - loudness(x))
    pk = np.abs(x).max()
    if pk > db(ceiling_db):  # gentle limiter: soft-knee tanh only on the overs
        k = db(ceiling_db)
        x = np.where(np.abs(x) > k * 0.7, np.sign(x) * (k * 0.7 + (k * 0.3) * np.tanh((np.abs(x) - k * 0.7) / (k * 0.3))), x)
    return x


def chord_swell(root, minor, dur=3.0, gain_db=-14):
    """A soft string-like chord (detuned, low-passed saws) that swells in and dies away."""
    ratios = [1, 6 / 5 if minor else 5 / 4, 3 / 2, 2]
    t = np.arange(int(dur * SR)) / SR
    s = np.zeros_like(t)
    for r in ratios:
        for det in (-0.004, 0.0, 0.005):
            f = root * r * (1 + det)
            ph = rng.uniform(0, 1)
            s += 2 * ((f * t + ph) % 1) - 1  # saw
    s = filt(s, 90, 1600 if minor else 2400)
    env = np.minimum(1, t / 0.35) * np.exp(-np.maximum(0, t - 0.4) * (1.6 if minor else 1.2))
    return s * env / len(ratios) * db(gain_db)


def final_pitch(x):
    """Fundamental of the last loud note (to tune the swell under a sting)."""
    e = envelope(x, 0.02)
    end = np.where(e > e.max() * db(-20))[0][-1]
    seg = x[max(0, end - 8192):end] * np.hanning(min(8192, end))
    X = np.abs(np.fft.rfft(seg, 65536)); f = np.fft.rfftfreq(65536, 1 / SR)
    band = (f > 100) & (f < 1000)
    return f[band][X[band].argmax()]


# ---- recipes --------------------------------------------------------------------------------
# Each returns a list of takes. Target levels are relative to each other: a hover sits far below
# a creature getting hit, a card being drawn below one hitting the table.

def T(pack, f):
    return trim(load(pack, f))


def card_draw():
    return [master(fade(filt(T("casino", f"card-slide-{i}.ogg"), 250, 9000)), -27) for i in (1, 2, 3, 4, 5, 6)]


def card_play():
    takes = []
    for i, k in zip((1, 2, 3, 4), (0, 1, 2, 3)):
        x = layer((T("casino", f"card-place-{i}.ogg"), 0, 0), (filt(T("impact", f"impactSoft_medium_00{k}.ogg"), 40, 900), -13, 0.004))
        takes.append(master(fade(x), -22))
    return takes


def land_drop():
    takes = []
    for i, k in zip((1, 2, 3, 4), (1, 2, 3, 4)):
        x = layer((T("casino", f"card-shove-{i}.ogg"), -3, 0),
                  (filt(T("impact", f"impactSoft_heavy_00{k}.ogg"), 35, 700), -8, 0.006),
                  (T("impact", f"impactWood_light_00{k}.ogg"), -16, 0.006))
        takes.append(master(fade(x, fout=0.08), -20))
    return takes


def spell_cast():
    takes = []
    for i, (s, p) in enumerate((("spell_01.ogg", 0), ("spell_02.ogg", 0), ("spell_01.ogg", -2))):
        x = layer((T("casino", f"card-place-{i + 1}.ogg"), -6, 0), (pitch(T("rpg80", s), p), 0, 0.03))
        takes.append(master(reverb(fade(x, fout=0.1), 0.3, 1.0), -21))
    return takes


def resolve():
    takes = []
    for g, p in (("item_gem_01.ogg", 0), ("item_gem_03.ogg", 0), ("item_gem_01.ogg", -3)):
        x = layer((pitch(T("rpg80", g), p), 0, 0), (filt(T("interface", "glass_001.ogg"), 300, None), -10, 0.01))
        takes.append(master(reverb(fade(x, fout=0.08), 0.45, 1.4), -23))
    return takes


def attack_whoosh():
    takes = []
    for i, c in zip((1, 2, 3), (1, 2, 3)):
        x = layer((T("rpg", f"cloth{c}.ogg"), -6, 0), (T("rpg", f"drawKnife{i}.ogg"), 0, 0.05))
        takes.append(master(reverb(fade(x, fout=0.08), 0.15, 0.6), -21))
    return takes


def hit_impact():
    takes = []
    for k in range(5):
        x = layer((T("impact", f"impactPunch_medium_00{k}.ogg"), 0, 0), (filt(T("impact", f"impactSoft_medium_00{k}.ogg"), 35, 500), -9, 0))
        takes.append(master(fade(x, fout=0.06), -18))
    return takes


def life_loss():
    takes = []
    for k in (0, 2, 4):
        x = layer((filt(T("impact", f"impactPunch_heavy_00{k}.ogg"), None, 5000), 0, 0), (filt(T("impact", f"impactSoft_heavy_00{k}.ogg"), 30, 400), -4, 0))
        takes.append(master(reverb(fade(x, fout=0.1), 0.12, 0.7, damp=2500), -18))
    return takes


def life_gain():
    takes = []
    for g, gl, p in (("item_gem_04.ogg", "glass_004.ogg", 0), ("item_gem_03.ogg", "glass_004.ogg", 2), ("item_gem_01.ogg", "glass_004.ogg", 4)):
        x = layer((pitch(T("rpg80", g), p), 0, 0), (filt(T("interface", gl), 500, None), -8, 0.06))
        takes.append(master(reverb(fade(x, fout=0.1), 0.5, 1.6), -23))
    return takes


def turn_start():
    takes = []
    for k in (0, 1):
        x = filt(T("impact", f"impactBell_heavy_00{k}.ogg"), 120, 3500)
        takes.append(master(reverb(fade(x, fout=0.2), 0.35, 1.8, damp=4000), -25))
    return takes


def button_click():
    return [master(fade(filt(T(p, f), 400, 9000), fout=0.01), -25) for p, f in (("interface", "select_001.ogg"), ("interface", "select_002.ogg"), ("ui", "click2.ogg"))]


def hover_tick():
    return [master(fade(filt(T("ui", f), 800, 8000), fout=0.01), -36) for f in ("rollover2.ogg", "rollover3.ogg", "rollover4.ogg")]


def sting(name, minor):
    x = T("jingles", name)
    root = final_pitch(x)
    while root > 220:
        root /= 2  # the swell sits an octave or two below the melody
    pad = chord_swell(root, minor, 3.2)
    y = layer((x, 0, 0), (pad, 0, 0.25 if not minor else 0.2))
    return master(reverb(fade(y, fout=0.4), 0.4, 2.2, damp=5000), -17)


def victory():
    return [sting("jingles_PIZZI02.ogg", False), sting("jingles_PIZZI04.ogg", False)]


def defeat():
    return [sting("jingles_PIZZI01.ogg", True), sting("jingles_PIZZI05.ogg", True)]


def card_fan():
    takes = [master(fade(filt(T("casino", "card-fan-1.ogg"), 200, 10000), fout=0.1), -24)]
    takes.append(master(fade(filt(T("casino", "card-shuffle.ogg"), 200, 10000)[: int(1.2 * SR)], fout=0.25), -25))
    return takes


SOUNDS = {
    "card_draw": card_draw, "card_play": card_play, "land_drop": land_drop, "spell_cast": spell_cast,
    "resolve": resolve, "attack_whoosh": attack_whoosh, "hit_impact": hit_impact, "life_loss": life_loss,
    "life_gain": life_gain, "turn_start": turn_start, "button_click": button_click, "hover_tick": hover_tick,
    "victory": victory, "defeat": defeat, "card_fan": card_fan,
}


def save(path, x):
    x = np.asarray(x, np.float64)
    ch = 2 if x.ndim == 2 else 1  # (n, 2) is interleaved L/R in C order
    pcm = (np.clip(x, -1, 1) * 32767).astype("<i2").tobytes()
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "s16le", "-ar", str(SR), "-ac", str(ch), "-i", "-",
                    "-c:a", "libvorbis", "-q:a", "5", path], input=pcm, check=True)


if __name__ == "__main__":
    fetch()
    os.makedirs(OUT, exist_ok=True)
    for name, fn in SOUNDS.items():
        for old in [f for f in os.listdir(OUT) if f == name + ".ogg" or (f.startswith(name + "_") and f[len(name) + 1:-4].isdigit())]:
            os.remove(os.path.join(OUT, old))
        takes = fn()
        for i, x in enumerate(takes, 1):
            save(os.path.join(OUT, f"{name}_{i}.ogg"), x)
        print(f"{name:14s} {len(takes)} takes, {max(len(t) for t in takes) / SR:.2f} s max")
