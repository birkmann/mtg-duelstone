#!/usr/bin/env python3
"""Curate protocol fixtures from autopilot recordings (`autopilot --record DIR`).

usage: curate_fixtures.py OUT.jsonl REC1.jsonl [REC2.jsonl ...]

Keeps every distinct non-`state` frame shape (one per input message template / prompt kind / frame
type, capped) plus a few `state` snapshots with their hidden libraries trimmed so the file stays small.
Lines are plain frames (one JSON object per line), which `EngineLink::replay` also accepts.
"""
import json, re, sys

out, recs = sys.argv[1], sys.argv[2:]
keep, seen, kinds = [], {}, {}

def trim_state(f):
    for p in f["game"]["players"]:
        z = p["zones"]
        z["library"] = z["library"][:3]
    return f

for rec in recs:
    states = []
    for line in open(rec):
        fr = json.loads(line)["frame"]
        t = fr["t"]
        if t == "state":
            states.append(fr)
            continue
        if t == "input":
            key = ("input", re.sub(r"\d+", "#", fr["message"])[:50], fr["ok"]["label"], fr["cancel"]["label"], bool(fr["selectable"]), bool(fr["trayCards"]))
        elif t == "prompt":
            key = ("prompt", fr["kind"], len(fr.get("options", [])) > 0, fr.get("min"), fr.get("max"))
        elif t == "events":
            key = ("events", tuple(sorted({i["kind"] for i in fr["items"]})))
        elif t == "log":
            key = ("log", tuple(sorted({e["type"] for e in fr["entries"]})))
        else:
            key = (t,)
        if key in seen:
            continue
        seen[key] = 1
        if t == "prompt":
            kinds[fr["kind"]] = kinds.get(fr["kind"], 0) + 1
            if kinds[fr["kind"]] > 4:
                continue
        keep.append(fr)
    # a few states per recording: middle and last
    if states:
        for idx in (len(states) // 2, len(states) - 1):
            keep.append(trim_state(states[idx]))

with open(out, "w") as f:
    for fr in keep:
        f.write(json.dumps(fr, separators=(",", ":")) + "\n")
print(len(keep), "frames,", sum(len(json.dumps(x)) for x in keep) // 1024, "KiB", kinds)
