#!/usr/bin/env python3
"""Helper for the Gnomish Relay spikes. See spikes/README.md.

  spike.py install   copy the test addons and make the test files (game closed)
  spike.py mutate    change the test files (game running, after /grfiles before)
  spike.py shot      read the newest screenshot and look for the test strip
  spike.py results   print the results that the addons saved
  spike.py remove    delete the test addons from the game folder
"""

import os
import shutil
import struct
import sys
from pathlib import Path

WOW = Path(os.environ.get(
    "WOW_DIR",
    Path.home() / "Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_",
))
ADDONS = WOW / "Interface" / "AddOns"
HERE = Path(__file__).resolve().parent
TEST_ADDONS = ["GRSpikeShot", "GRSpikeFiles", "GRSpikeSlot1", "GRSpikeSlot2", "GRSpikeLate"]

# The same silent sound that wow-claude uses: 8 kHz, 8-bit, mono, 80 samples.
SAMPLES = 80
SILENT_WAV = (
    b"RIFF" + struct.pack("<I", 36 + SAMPLES) + b"WAVE"
    + b"fmt " + struct.pack("<IHHIIHH", 16, 1, 1, 8000, 8000, 1, 8)
    + b"data" + struct.pack("<I", SAMPLES) + bytes([128]) * SAMPLES
)


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_bytes(data if isinstance(data, bytes) else data.encode())
    tmp.replace(path)


def slot_addon(name, body):
    toc = f"## Interface: 16001\n## Title: {name}\n## LoadOnDemand: 1\n## Dependencies: GRSpikeFiles\n\nInbox.lua\n"
    write(ADDONS / name / f"{name}.toc", toc)
    write(ADDONS / name / "Inbox.lua", body)


def install():
    if not WOW.is_dir():
        sys.exit(f"WoW folder not found: {WOW}\nSet WOW_DIR to the _classic_beta_ folder.")
    ADDONS.mkdir(parents=True, exist_ok=True)
    for name in ("GRSpikeShot", "GRSpikeFiles"):
        shutil.rmtree(ADDONS / name, ignore_errors=True)
        shutil.copytree(HERE / "addons" / name, ADDONS / name)
    shutil.rmtree(ADDONS / "GRSpikeLate", ignore_errors=True)

    snd = ADDONS / "GRSpikeFiles" / "snd"
    write(snd / "empty.wav", b"")
    write(snd / "valid.wav", SILENT_WAV)
    write(snd / "flip_on.wav", b"")
    write(snd / "flip_off.wav", SILENT_WAV)
    (snd / "late.wav").unlink(missing_ok=True)

    slot_addon("GRSpikeSlot1", 'GRSpikeSlotData = GRSpikeSlotData or {}\nGRSpikeSlotData.slot1 = "launch"\n')
    slot_addon("GRSpikeSlot2", "GRSpikeSlotData = GRSpikeSlotData or {}\n"
               "GRSpikeSlotData.slot2count = (GRSpikeSlotData.slot2count or 0) + 1\n")
    print(f"Installed into {ADDONS}")
    print("Next: start WoW, enable the GR Spike addons, log in, run /grfiles before.")


def mutate():
    snd = ADDONS / "GRSpikeFiles" / "snd"
    write(snd / "flip_on.wav", SILENT_WAV)
    write(snd / "flip_off.wav", b"")
    write(snd / "late.wav", SILENT_WAV)
    write(ADDONS / "GRSpikeSlot1" / "Inbox.lua",
          'GRSpikeSlotData = GRSpikeSlotData or {}\nGRSpikeSlotData.slot1 = "mutated"\n')
    slot_addon("GRSpikeLate", "GRSpikeSlotData = GRSpikeSlotData or {}\nGRSpikeSlotData.late = true\n")
    print("Files changed. Next: /grfiles after, then /reload, then /grfiles reload.")


def expected_cell(row, i):
    return i % 8 if row == 0 else 7 - i % 8


def read_cell(px, x, y):
    r, g, b = px[x, y][:3]
    return (r >= 128) * 4 + (g >= 128) * 2 + (b >= 128)


def score(px, size, pitch_x, pitch_y):
    width, height = size
    hits = total = 0
    for row in range(2):
        y = int((row + 0.5) * pitch_y)
        for i in range(200):
            x = int((i + 0.5) * pitch_x)
            if x >= width or y >= height:
                break
            total += 1
            hits += read_cell(px, x, y) == expected_cell(row, i)
    return hits, total


def find_strip(img):
    """Search the cell size. UI scaling makes it fractional, for example 3.875 px."""
    px = img.load()
    best = (0, None)
    for pitch_y in (p / 8 for p in range(24, 65)):
        # The row check alone picks the height: row 2 is row 1 backwards.
        for pitch_x in (p / 200 for p in range(600, 1601)):
            hits, total = score(px, img.size, pitch_x, pitch_y)
            if hits > best[0]:
                best = (hits, (pitch_x, pitch_y, total))
            if hits == total == 400:
                return best
    return best


def shot():
    from PIL import Image

    folder = WOW / "Screenshots"
    files = sorted(folder.glob("*"), key=lambda p: p.stat().st_mtime) if folder.is_dir() else []
    if not files:
        sys.exit(f"No screenshots in {folder}")
    path = files[-1]
    img = Image.open(path).convert("RGB")
    print(f"{path.name}: {img.size[0]}x{img.size[1]}, format {path.suffix}")
    hits, where = find_strip(img)
    if not where:
        sys.exit("No strip found.")
    pitch_x, pitch_y, total = where
    print(f"Best grid: cell {pitch_x:.3f} x {pitch_y:.3f} px: {hits} of {total} cells correct.")
    print("PASS: the strip survives the screenshot." if hits == total else "FAIL: some cells are wrong.")
    corner = img.crop((0, 0, min(img.size[0], 820), min(img.size[1], 40)))
    out = HERE / "last-strip.png"
    corner.save(out)
    print(f"Top-left corner saved to {out}")


def results():
    accounts = sorted((WOW / "WTF" / "Account").glob("*/SavedVariables"))
    if not accounts:
        sys.exit("No saved variables yet. Log out or /reload so that WoW writes them.")
    for sv in accounts:
        for name in ("GRSpikeFiles.lua", "GRSpikeShot.lua"):
            f = sv / name
            if f.exists():
                print(f"== {f}")
                print(f.read_text())


def remove():
    for name in TEST_ADDONS:
        shutil.rmtree(ADDONS / name, ignore_errors=True)
    print("Test addons removed.")


if __name__ == "__main__":
    commands = {"install": install, "mutate": mutate, "shot": shot, "results": results, "remove": remove}
    if len(sys.argv) != 2 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    commands[sys.argv[1]]()
