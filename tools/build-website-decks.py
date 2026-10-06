#!/usr/bin/env python3
"""Writes website/assets/decks.json, the data behind the website's deck explorer (#decks).

The website is static (Vercel) and has no engine to ask, so everything the explorer shows is
baked here from the decks that ship with the game (decks/) and the Scryfall bulk data that
tools/fetch-offline.sh leaves in offline/img/.bulk/. Card images are not copied: the page links
Scryfall's image CDN by printing id, like the game's own image fetch.

Cards are stored once in a table and decks refer to them by index, so ~700 decks stay a few
hundred KB. Run it again after the archetype decks are regenerated:

    python3 tools/build-website-decks.py
"""
import gzip
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DECKS = ROOT / "decks"
BULK = ROOT / "offline" / "img" / ".bulk"
OUT = ROOT / "website" / "assets" / "decks.json"

# Tab order on the page. `dirs` are the deck folders that feed each format: Forge's metagame
# archetypes (ArchetypeDecks) and real MTGO lists (TournamentDecks), whichever exist.
CONSTRUCTED = [
    ("standard", "Standard", "The newest sets only. A fresh card pool every year."),
    ("pioneer", "Pioneer", "Every set since Return to Theros, no rotation."),
    ("modern", "Modern", "Every set since Eighth Edition. Fast, deep and diverse."),
    ("legacy", "Legacy", "Every card ever printed, with a ban list."),
    ("vintage", "Vintage", "Every card ever printed, the Power Nine restricted."),
    ("pauper", "Pauper", "Commons only, from any set."),
]
FORMATS = [(fid, name, blurb, [(f"archetypes/{fid}", "meta"), (f"tournament/{fid}", "event")]) for fid, name, blurb in CONSTRUCTED] + [
    ("commander", "Commander", "100 cards, one of each, led by a legendary creature.", [("commander", "starter")]),
    ("starter", "Starter", "Simple decks that come with the game, good for a first match.", [("constructed", "starter")]),
]

CATEGORIES = ["Creature", "Planeswalker", "Battle", "Instant", "Sorcery", "Artifact", "Enchantment", "Land"]
RARITY = {"common": "c", "uncommon": "u", "rare": "r", "mythic": "m", "special": "r", "bonus": "r"}
LINE = re.compile(r"^(\d+)\s+([^|]+?)(?:\|([^|\s]+))?(?:\|\[([^\]]+)\])?\s*$")


def parse_dck(path):
    meta, sections, cur = {}, {"main": [], "side": [], "cmd": []}, None
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith("["):
            tag = line.strip("[]").lower()
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


def main():
    if not (BULK / "default_cards.jsonl.gz").exists():
        sys.exit(f"missing {BULK}/default_cards.jsonl.gz: run tools/fetch-offline.sh --images-only first")

    raw_decks = []  # (format id, source, path, meta, sections)
    wanted_prints, wanted_names = set(), set()
    for fid, _, _, dirs in FORMATS:
        for sub, source in dirs:
            for path in sorted((DECKS / sub).glob("*.dck")):
                meta, sections = parse_dck(path)
                raw_decks.append((fid, source, path, meta, sections))
                for rows in sections.values():
                    for _, name, s, num in rows:
                        wanted_names.add(name.lower())
                        if s and num:
                            wanted_prints.add((s, num))

    # One pass over each bulk file, keeping only what the decks use.
    by_print, by_name = {}, {}

    def names_of(card):
        yield card["name"].lower()
        for f in card.get("card_faces") or []:
            yield f["name"].lower()

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
        key = name.lower()
        if key in card_index:
            return card_index[key]
        oracle = by_name.get(key)
        printing = by_print.get((set_code, num))
        if printing and printing["name"].lower() != key and key not in [n for n in names_of(printing)]:
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

    formats_out = []
    for fid, fname, blurb, _ in FORMATS:
        decks_out = []
        for dfid, source, path, meta, sections in raw_decks:
            if dfid != fid:
                continue
            out = {"id": path.stem, "name": meta.get("name", path.stem), "src": source}
            note = meta.get("description") or meta.get("comment") or ""
            if source == "meta":
                m = re.search(r"(\d+)\s+tournament deck", note)
                out["n"] = int(m.group(1)) if m else 0
            elif source == "event":
                parts = [p.strip() for p in note.split("·")]
                out["event"], out["date"] = parts[0], parts[1] if len(parts) > 1 else ""
                out["place"] = parts[2] if len(parts) > 2 else ""  # "#11" in a Challenge, "5-0" in a League
                if any(d["id"] == path.stem for d in decks_out):
                    out["id"] = f"{path.stem}-{source}"
            else:
                out["note"] = note
            for sec in ("cmd", "main", "side"):
                merged = {}
                for n, name, s, num in sections[sec]:
                    ref = card_ref(name, s, num)
                    if ref is not None:
                        merged[ref] = merged.get(ref, 0) + n
                if merged:
                    out[sec] = [[ref, n] for ref, n in merged.items()]
            decks_out.append(out)
        formats_out.append({"id": fid, "name": fname, "blurb": blurb, "decks": decks_out})

    data = {
        "v": 1,
        "cardFields": ["name", "cost", "mv", "cat", "colors", "rarity", "img", "type"],
        "categories": CATEGORIES,
        "cards": cards,
        "formats": formats_out,
    }
    OUT.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    total = sum(len(f["decks"]) for f in formats_out)
    print(f"{OUT.relative_to(ROOT)}: {total} decks, {len(cards)} cards, {OUT.stat().st_size // 1024} KB")
    if missing:
        print(f"no Scryfall data for {len(missing)} card(s), left out: {', '.join(sorted(missing))}", file=sys.stderr)


if __name__ == "__main__":
    main()
