#!/usr/bin/env python3
"""Writes website/assets/decks.json, the data behind the website's deck explorer (#decks).

The explorer lists exactly what the game offers: this script starts the engine jar (or uses
--engine URL), asks it for every format's playable decks (GET /api/decks, the same lists the deck
screen shows, legality checked by Forge) and reads each deck's .dck from wherever the ref points:
decks/ (starters, archetypes, tournament and community decks) or Forge's res/ (theme decks,
Commander precons, quest and genetic-AI decks). Card data comes from the Scryfall bulk files that
tools/fetch-offline.sh leaves in offline/img/.bulk/; images are not copied, the page links
Scryfall's image CDN by printing id, like the game's own image fetch.

Cards are stored once in a table, decks once in a list (Legacy and Vintage share most of theirs),
and formats refer to decks by index. Run it again after the deck folders change:

    python3 tools/build-website-decks.py [--engine http://127.0.0.1:8080]
"""
import gzip
import json
import os
import re
import shutil
import subprocess
import sys
import time
import unicodedata
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DECKS = ROOT / "decks"
RES = ROOT / "forge" / "forge-gui" / "res"
BULK = ROOT / "offline" / "img" / ".bulk"
JAR = ROOT / "engine" / "target" / "forge-ws-server.jar"
OUT = ROOT / "website" / "assets" / "decks.json"
PORT = 18093

# Tabs: (id, name, blurb, engine format, legality). "casual" holds the 60-card decks no listed
# legality accepts; "starter" the simple decks of decks/constructed (they also appear in the tabs
# they are legal in).
TABS = [
    ("standard", "Standard", "The newest sets only. A fresh card pool every year.", "constructed", "standard"),
    ("pioneer", "Pioneer", "Every set since Return to Theros, no rotation.", "constructed", "pioneer"),
    ("modern", "Modern", "Every set since Eighth Edition. Fast, deep and diverse.", "constructed", "modern"),
    ("legacy", "Legacy", "Every card ever printed, with a ban list.", "constructed", "legacy"),
    ("vintage", "Vintage", "Every card ever printed, the Power Nine restricted.", "constructed", "vintage"),
    ("pauper", "Pauper", "Commons only, from any set.", "constructed", "pauper"),
    ("commander", "Commander", "100 cards, one of each, led by a legendary creature.", "commander", None),
    ("brawl", "Brawl", "Commander on a Standard card pool: 60 cards, 25 life, one-on-one.", "brawl", None),
    ("oathbreaker", "Oathbreaker", "60 cards led by a planeswalker and its signature spell.", "oathbreaker", None),
    ("casual", "Casual", "60-card decks for kitchen-table play: no format takes them, the game does.", "constructed", None),
    ("starter", "Starter", "Simple decks that come with the game, good for a first match.", "constructed", None),
]

CATEGORIES = ["Creature", "Planeswalker", "Battle", "Instant", "Sorcery", "Artifact", "Enchantment", "Land"]
RARITY = {"common": "c", "uncommon": "u", "rare": "r", "mythic": "m", "special": "r", "bonus": "r"}
# "4 Name", "4 Name|SET", "4 Name|SET|2" (art index), "4 Name|SET|[123]" (collector number), and
# Forge's trailing flags ("|Foil", "|Marked…") in any mix
LINE = re.compile(r"^(\d+)\s+([^|]+?)(?:\|([^|\s]+))?(?:\|\d+)?(?:\|\[([^\]]+)\])?(?:\|.*)?\s*$")
UNPARSED = []


def norm(name):
    """Match key for card names: Forge writes "Lim-Dul" for Scryfall's "Lim-Dûl" and marks its quest
    variants with a trailing "+" ("Krenko, Mob Boss+"), which play as the printed card."""
    folded = unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode()
    return folded.lower().rstrip("+").strip()


def parse_dck(path, header_only=False):
    meta, sections, cur = {}, {"main": [], "side": [], "cmd": []}, None
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith("["):
            tag = line.strip("[]").lower()
            if header_only and tag != "metadata" and meta:
                break
            cur = {"main": "main", "sideboard": "side", "commander": "cmd"}.get(tag, "meta" if tag == "metadata" else None)
            continue
        if cur == "meta":
            k, _, v = line.partition("=")
            meta[k.strip().lower()] = v.strip()
        elif cur:
            m = LINE.match(line)
            if m:
                n, name, set_code, num = m.groups()
                sections[cur].append((int(n), name.strip(), (set_code or "").lower(), num or ""))
            elif not header_only:
                UNPARSED.append(f"{path.name}: {line}")
    return meta, sections


def front(card):
    """Front face for multi-faced cards, whose top level has no cost, image or single type."""
    faces = card.get("card_faces") or []
    return faces[0] if faces and card.get("layout") not in ("split", "flip") else card


def category(type_line):
    t = type_line.split("//")[0]
    if "Creature" in t:
        return "Creature"
    if "Land" in t:
        return "Land"
    for c in CATEGORIES:
        if c in t:
            return c
    return "Artifact" if "Artifact" in type_line else "Sorcery"


def image_id(card):
    # Scryfall's CDN path is derived from the printing id; cards without art (very new
    # previews) fall back to whatever printing the oracle file chose.
    return card["id"] if card.get("image_status") not in (None, "missing", "placeholder") else None


def engine_lists(base):
    """{tab id: [deck info]} from the engine, playable decks only."""
    def get(fmt, legality):
        q = {"format": fmt}
        if legality:
            q["legality"] = legality
        with urllib.request.urlopen(f"{base}/api/decks?{urllib.parse.urlencode(q)}", timeout=300) as r:
            return [d for d in json.load(r) if not d.get("problem") and d["ref"] != "random" and not d["ref"].startswith(("user:", "file:"))]

    lists, casual = {}, None
    for tid, _, _, fmt, legality in TABS:
        if tid in ("casual", "starter"):
            casual = casual or get("constructed", None)
            continue
        lists[tid] = get(fmt, legality)
    legal = {d["ref"] for tid, _, _, fmt, l in TABS if l for d in lists[tid]}
    lists["casual"] = [d for d in casual if d["ref"] not in legal and not d["ref"].startswith("starter:")]
    lists["starter"] = [d for d in casual if d["ref"].startswith("starter:")]
    return lists


def start_engine():
    if not JAR.exists():
        sys.exit(f"missing {JAR}: build it with (cd engine && mvn -q -B package)")
    java = shutil.which("java") or (Path(os.environ.get("JAVA_HOME", "")) / "bin" / "java")
    proc = subprocess.Popen([str(java), "-Xmx3g", "-jar", str(JAR), "--port", str(PORT), "--assets", str(RES.parent)],
                            cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    base = f"http://127.0.0.1:{PORT}"
    for _ in range(240):
        try:
            urllib.request.urlopen(f"{base}/api/health", timeout=2).close()
            return proc, base
        except OSError:
            if proc.poll() is not None:
                sys.exit("the engine exited during startup (source tools/env.sh for JDK 17?)")
            time.sleep(0.5)
    proc.kill()
    sys.exit("the engine did not come up in 2 minutes")


def named_decks(folder):
    """Forge names theme decks and precons by their Name= (else the file stem)."""
    out = {}
    for path in sorted(folder.glob("*.dck")):
        meta, _ = parse_dck(path, header_only=True)
        out.setdefault(meta.get("name", path.stem), path)
        out.setdefault(path.stem, path)
    return out


def deck_file(ref, fmt, themes, precons):
    kind, _, rest = ref.partition(":")
    if kind == "starter":
        return DECKS / fmt / f"{rest}.dck"
    if kind in ("archetype", "tournament"):
        return DECKS / ("archetypes" if kind == "archetype" else "tournament") / f"{rest}.dck"
    if kind == "community":
        return DECKS / "community" / f"{rest}.dck"
    if kind == "forge":
        return RES / f"{rest}.dck"
    if kind == "theme":
        return themes.get(rest)
    if kind == "precon":
        return precons.get(rest)
    return None


def source_of(ref, meta):
    kind, _, rest = ref.partition(":")
    if kind == "archetype":
        return "meta"
    if kind == "tournament":
        return "event"
    if kind == "community":
        return "gen" if "play data" in (meta.get("comment") or "") else "creator"
    if kind == "forge":
        return "genetic" if rest.startswith("geneticaidecks/") else "quest"
    return kind  # starter, precon, theme


def main():
    if not (BULK / "default_cards.jsonl.gz").exists():
        sys.exit(f"missing {BULK}/default_cards.jsonl.gz: run tools/fetch-offline.sh --images-only first")
    if not RES.is_dir():
        sys.exit(f"missing {RES}: run tools/setup-forge.sh")

    args = sys.argv[1:]
    proc = None
    if "--engine" in args:
        base = args[args.index("--engine") + 1].rstrip("/")
    else:
        proc, base = start_engine()
    try:
        print("asking the engine for every format's decks (a cold deck-library index takes ~15 s)…", file=sys.stderr)
        lists = engine_lists(base)
    finally:
        if proc:
            proc.terminate()

    themes, precons = named_decks(RES / "quest" / "precons"), named_decks(RES / "quest" / "commanderprecons")
    raw_decks = {}  # ref → (source, path, info, meta, sections), each deck read once
    unreadable = []
    wanted_prints, wanted_names = set(), set()
    for tid, _, _, fmt, _ in TABS:
        for info in lists[tid]:
            ref = info["ref"]
            if ref in raw_decks:
                continue
            path = deck_file(ref, fmt, themes, precons)
            if not path or not path.is_file():
                unreadable.append(ref)
                continue
            meta, sections = parse_dck(path)
            raw_decks[ref] = (source_of(ref, meta), path, info, meta, sections)
            for rows in sections.values():
                for _, name, s, num in rows:
                    wanted_names.add(norm(name))
                    if s and num:
                        wanted_prints.add((s, num))
    if unreadable:
        print(f"no .dck found for {len(unreadable)} ref(s), left out: {', '.join(unreadable[:10])}", file=sys.stderr)

    # One pass over each bulk file, keeping only what the decks use.
    by_print, by_name = {}, {}

    def names_of(card):
        yield norm(card["name"])
        for f in card.get("card_faces") or []:
            yield norm(f["name"])

    with gzip.open(BULK / "default_cards.jsonl.gz", "rt", encoding="utf-8") as f:
        for line in f:
            card = json.loads(line)
            key = (card["set"], card["collector_number"])
            if key in wanted_prints and card.get("lang") == "en":
                by_print[key] = card
    with gzip.open(BULK / "oracle_cards.jsonl.gz", "rt", encoding="utf-8") as f:
        for line in f:
            card = json.loads(line)
            if card.get("layout") in ("art_series", "token", "double_faced_token", "emblem"):
                continue
            for n in names_of(card):
                if n in wanted_names and n not in by_name:
                    by_name[n] = card

    cards, card_index, missing = [], {}, set()

    def card_ref(name, set_code, num):
        key = norm(name)
        if key in card_index:
            return card_index[key]
        oracle = by_name.get(key)
        printing = by_print.get((set_code, num))
        if printing and key not in list(names_of(printing)):
            printing = None  # Forge and Scryfall disagree on that set code's numbering
        if not oracle and not printing:
            missing.add(name)
            card_index[key] = None
            return None
        base = oracle or printing
        face = front(base)
        colors = face.get("colors", base.get("colors")) or []
        img = (printing and image_id(printing)) or image_id(base)
        cards.append([
            name,
            face.get("mana_cost", base.get("mana_cost", "")),
            int(base.get("cmc") or 0),
            CATEGORIES.index(category(base.get("type_line", ""))),
            "".join(c for c in "WUBRG" if c in colors),
            RARITY.get((printing or base).get("rarity"), "c"),
            img or "",
            face.get("type_line", base.get("type_line", "")),
        ])
        card_index[key] = len(cards) - 1
        return card_index[key]

    decks_out, index, ids = [], {}, set()
    for ref, (source, path, info, meta, sections) in raw_decks.items():
        # the engine's name: for Forge's own decks it says what the deck plays ("Simic Adventure", not "Owlman")
        out = {"name": info.get("name") or meta.get("name", path.stem), "src": source}
        note = info.get("description") or meta.get("description") or meta.get("comment") or ""
        if source == "precon":
            # "Abzan Armor [TDC] [2025]": the set code and year become the subtitle
            m = re.match(r"^(.*?)\s*\[([^\]]+)\]\s*\[(\d{4})\]$", out["name"])
            if m:
                out["name"], note = m.group(1), f"Precon · {m.group(2)} · {m.group(3)}"
                out["date"] = m.group(3)
        if source == "meta":
            m = re.search(r"(\d+)\s+tournament deck", note)
            out["n"] = int(m.group(1)) if m else 0
        elif source == "event":
            parts = [p.strip() for p in note.split("·")]
            if parts[0].startswith("Forge quest"):
                # "Forge quest duel · <set> · <opponent>": a curated list, no date or placing
                out["event"], out["date"], out["place"] = " · ".join(parts[:2]), "", ""
            else:
                out["event"], out["date"] = parts[0], parts[1] if len(parts) > 1 else ""
                out["place"] = parts[2] if len(parts) > 2 else ""  # "#11" in a Challenge, "5-0" in a League
        elif note:
            out["note"] = note if len(note) <= 120 else note[:119].rstrip() + "…"
        # #decks/<format>/<id> links: a readable slug of the name, numbered when names repeat
        slug = re.sub(r"[^a-z0-9]+", "-", norm(out["name"])).strip("-") or "deck"
        out["id"], k = slug, 2
        while out["id"] in ids:
            out["id"], k = f"{slug}-{k}", k + 1
        ids.add(out["id"])
        for sec in ("cmd", "main", "side"):
            merged = {}
            for n, name, s, num in sections[sec]:
                cref = card_ref(name, s, num)
                if cref is not None:
                    merged[cref] = merged.get(cref, 0) + n
            if merged:
                # flat [card, count, card, count, …]: ~11k decks, every byte counts
                out[sec] = [x for pair in merged.items() for x in pair]
        index[ref] = len(decks_out)
        decks_out.append(out)

    formats_out = []
    for tid, name, blurb, _, _ in TABS:
        ids = [index[d["ref"]] for d in lists[tid] if d["ref"] in index]
        formats_out.append({"id": tid, "name": name, "blurb": blurb, "decks": ids})

    data = {
        "v": 2,
        "cardFields": ["name", "cost", "mv", "cat", "colors", "rarity", "img", "type"],
        "categories": CATEGORIES,
        "cards": cards,
        "decks": decks_out,
        "formats": formats_out,
    }
    OUT.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    per = ", ".join(f"{f['id']} {len(f['decks'])}" for f in formats_out)
    print(f"{OUT.relative_to(ROOT)}: {len(decks_out)} decks ({per}), {len(cards)} cards, {OUT.stat().st_size // 1024} KB")
    if UNPARSED:
        print(f"{len(UNPARSED)} deck line(s) not understood, e.g. {UNPARSED[:3]}", file=sys.stderr)
    if missing:
        print(f"no Scryfall data for {len(missing)} card(s), left out: {', '.join(sorted(missing))}", file=sys.stderr)


if __name__ == "__main__":
    main()
