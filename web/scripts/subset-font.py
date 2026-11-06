#!/usr/bin/env python3
"""Regenerate the Maple Mono web subset used by the Hub.

The full font is not committed to the repository. Place it at
`web/public/static/fonts/MapleMono-Medium.woff2` (or pass --source) and run:

    python3 web/scripts/subset-font.py

Requires `fonttools` with WOFF2 support (`pip install fonttools brotli`).
The subset keeps ASCII, Latin-1 punctuation, common symbols/arrows and
Chinese/English punctuation, but deliberately drops CJK ideographs; CJK text
falls back to the system font stack in CSS.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from fontTools import subset

# ASCII printable range.
ASCII = "".join(chr(code) for code in range(0x20, 0x7F))
# Latin-1 / common typography requested by the design.
LATIN1 = "¥°·×÷©®±µ¼½¾"
# Math, currency, arrows and quotation marks commonly used by the UI.
SYMBOLS = (
    "≈≤≥℃€‰′″←→↑↓↔↕⇒⇐•…—–“”‘’«»‹›§¶†‡"
    "±×÷−∞√∑∏∫∆"
)
# Chinese/English punctuation; ideographs remain on the system font.
PUNCTUATION = "，。！？；：、（）【】「」『』《》〈〉…—·"


def main() -> int:
    repo = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        default=repo / "web/public/static/fonts/MapleMono-Medium.woff2",
        help="full Maple Mono WOFF2 source (not committed)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=repo / "web/public/fonts/MapleMono-Medium-subset.woff2",
        help="subset output path",
    )
    args = parser.parse_args()

    if not args.source.is_file():
        print(f"source font not found: {args.source}", file=sys.stderr)
        return 1

    options = subset.Options()
    options.flavor = "woff2"
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.notdef_outline = True
    font = subset.load_font(str(args.source), options)
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(text=ASCII + LATIN1 + SYMBOLS + PUNCTUATION)
    subsetter.subset(font)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    subset.save_font(font, str(args.output), options)
    print(f"wrote {args.output} ({args.output.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
