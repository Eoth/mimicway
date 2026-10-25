#!/usr/bin/env python3
"""Turns the frames of the README demo into one animated WebP per language.

    python3 scripts/make-demo-tour.py [frames directory]   (default: frontend/test-results/demo-tour)

frontend/e2e/demo-tour.spec.js writes the frames, <frames>/<language>/01.png and on, and their captions in
<frames>/captions.json, when run by `npm run docs:demo`. Each frame is scaled to 960 px, its caption printed on a band
below it, and the frames are written to docs/<language>/screenshots/demo-tour.webp, one every few seconds: the screens
of a video, at the weight of one or two screenshots. A file over BUDGET bytes is refused rather than published, as is
a language with another number of frames than captions.

Needs Pillow (`python3 -m pip install --user Pillow`), with its FreeType support for the caption font.
"""
import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
WIDTH = 960
BAND = 48
BUDGET = 150_000
FRAME_MS = 3_000
LAST_FRAME_MS = 5_000
# Phasme: bark 900 under lichen 0, the contrast of the interface's dark surfaces.
BAND_COLOR = (0x35, 0x28, 0x1C)
TEXT_COLOR = (0xFB, 0xFC, 0xF9)


def composed(frame_path, number, total, caption, font):
    shot = Image.open(frame_path).convert("RGB")
    height = round(shot.height * WIDTH / shot.width)
    shot = shot.resize((WIDTH, height), Image.LANCZOS)
    image = Image.new("RGB", (WIDTH, height + BAND), BAND_COLOR)
    image.paste(shot, (0, 0))
    draw = ImageDraw.Draw(image)
    draw.text((20, height + BAND // 2), f"{number}/{total}   {caption}", font=font, fill=TEXT_COLOR, anchor="lm")
    return image


def main():
    frames_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "frontend" / "test-results" / "demo-tour"
    captions = json.loads((frames_dir / "captions.json").read_text(encoding="utf-8"))
    font = ImageFont.load_default(size=20)
    problems = []
    for language_dir in sorted(p for p in frames_dir.iterdir() if p.is_dir()):
        language = language_dir.name
        frames = sorted(language_dir.glob("*.png"))
        if len(frames) != len(captions):
            problems.append(f"{language}: {len(frames)} frames for {len(captions)} captions")
            continue
        images = [
            composed(frame, i + 1, len(frames), captions[i][language], font) for i, frame in enumerate(frames)
        ]
        out = ROOT / "docs" / language / "screenshots" / "demo-tour.webp"
        durations = [FRAME_MS] * (len(images) - 1) + [LAST_FRAME_MS]
        images[0].save(
            out, save_all=True, append_images=images[1:], duration=durations, loop=0, quality=70, method=6
        )
        size = out.stat().st_size
        print(f"{out.relative_to(ROOT)}: {len(images)} frames, {size} bytes")
        if size > BUDGET:
            problems.append(f"{out.relative_to(ROOT)}: {size} bytes, over the budget of {BUDGET}")
    if problems:
        print("\n".join(problems), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
