"""Build a Windows ZIP with maximum DEFLATE compression for executables."""

import argparse
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZIP_STORED, ZipFile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    archive = ZipFile(args.archive, "x", compression=ZIP_DEFLATED, compresslevel=9)
    try:
        with archive:
            for path in sorted(args.bundle.rglob("*")):
                if path.is_file():
                    method = ZIP_STORED if path.suffix == ".zst" else ZIP_DEFLATED
                    archive.write(path, path.relative_to(args.bundle.parent), compress_type=method)
    except BaseException:
        args.archive.unlink()
        raise


if __name__ == "__main__":
    main()
