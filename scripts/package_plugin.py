#!/usr/bin/env python3
"""Export the plugin binary and SHA-256 sidecar from a cargo-dist archive."""
import argparse
import hashlib
from pathlib import Path
import tarfile

LABELS = {
    "x86_64-unknown-linux-musl": "x86_64-linux",
    "aarch64-unknown-linux-musl": "aarch64-linux",
    "aarch64-apple-darwin": "aarch64-macos",
}


def package(archive_path, target, output):
    label = LABELS[target]
    with tarfile.open(archive_path, "r:*") as archive:
        entries = [entry for entry in archive.getmembers()
                   if entry.isfile() and Path(entry.name).name == "dm-installer"]
        if len(entries) != 1:
            raise ValueError("archive must contain exactly one regular dm-installer binary")
        binary = archive.extractfile(entries[0]).read()
    if not binary:
        raise ValueError("plugin binary is empty")
    output.mkdir(parents=True, exist_ok=True)
    name = f"dm-installer-{label}"
    destination = output / name
    destination.write_bytes(binary)
    destination.chmod(0o755)
    (output / f"{name}.sha256").write_text(
        f"{hashlib.sha256(binary).hexdigest()}  {name}\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("target", choices=LABELS)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    package(args.archive, args.target, args.output)
