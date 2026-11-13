"""Create the two shared, losslessly compressed fallback fonts."""

import argparse
from compression import zstd
from pathlib import Path

FONT_SOURCE = Path(__file__).resolve().parents[1] / "assets" / "fonts"
FONT_NAMES = ("NotoSansSC-Regular.ttf", "NotoSerifSC-SemiBold.ttf")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    for name in FONT_NAMES:
        source = (FONT_SOURCE / name).read_bytes()
        compressed = zstd.compress(source, level=19)
        if len(compressed) >= len(source):
            raise ValueError(f"Compressed font is larger than its source: {name}")
        (args.destination / f"{name}.zst").write_bytes(compressed)
        # Remove the old copy when rebuilding a bundle in place.
        (args.destination / name).unlink(missing_ok=True)


if __name__ == "__main__":
    main()
