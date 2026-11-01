#!/usr/bin/env python3
"""Split a verified Qt source ZIP into independently downloadable chunks."""
from __future__ import annotations
import argparse
import hashlib
from pathlib import Path

def digest_file(path: Path) -> tuple[int, str]:
    h = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            size += len(block)
            h.update(block)
    return size, h.hexdigest()

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--chunk-mib", type=int, default=250)
    args = parser.parse_args()
    if args.chunk_mib <= 0:
        parser.error("--chunk-mib must be positive")
    source = args.source.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    size, source_hash = digest_file(source)
    expected_hash = "a72e89c33726d742e67edf494abf7b625eaffc756673221c9823322ecd1a32d1"
    if size != 604643023 or source_hash != expected_hash:
        raise SystemExit(f"Source ZIP mismatch: {size} bytes, SHA256 {source_hash}")
    chunk_bytes = args.chunk_mib * 1024 * 1024
    chunks: list[tuple[Path, int, str]] = []
    with source.open("rb") as inp:
        index = 1
        remaining = size
        while remaining:
            wanted = min(chunk_bytes, remaining)
            target = output / f"{source.name}.part{index:03d}"
            h = hashlib.sha256()
            written = 0
            with target.open("wb") as out:
                while written < wanted:
                    block = inp.read(min(1024 * 1024, wanted - written))
                    if not block:
                        raise IOError(f"Unexpected EOF writing {target}")
                    out.write(block)
                    h.update(block)
                    written += len(block)
            chunks.append((target, written, h.hexdigest()))
            print(f"{target.name}: {written} bytes SHA256 {h.hexdigest()}", flush=True)
            remaining -= written
            index += 1
    # Prove concatenating the parts produces the original archive byte-for-byte.
    rebuilt = hashlib.sha256()
    rebuilt_size = 0
    for target, part_size, part_hash in chunks:
        actual_size, actual_hash = digest_file(target)
        if (actual_size, actual_hash) != (part_size, part_hash):
            raise ValueError(f"Chunk integrity failure: {target}")
        rebuilt_size += actual_size
        with target.open("rb") as stream:
            while block := stream.read(1024 * 1024):
                rebuilt.update(block)
    if rebuilt_size != size or rebuilt.hexdigest() != source_hash:
        raise ValueError("Chunk concatenation does not match official Qt source ZIP")
    lines = ["# Qt 6.12.0 source ZIP chunks", "", f"Original size: {size} bytes",
             f"Original SHA-256: {source_hash}", "", "| Part | Bytes | SHA-256 |",
             "|---|---:|---|"]
    for target, part_size, part_hash in chunks:
        lines.append(f"| {target.name} | {part_size} | {part_hash} |")
    lines += ["", "Concatenate the parts in lexical order with binary mode:", "",
              "```cmd", "copy /b " + "+".join(p.name for p, _, _ in chunks) +
              " qt-everywhere-src-6.12.0.zip", "```", ""]
    (output / "QT_CHUNKS.md").write_text("\n".join(lines), encoding="utf-8")
    with (output / "QT_CHUNKS_SHA256SUMS.txt").open("w", encoding="ascii") as sums:
        for target, _, part_hash in chunks:
            sums.write(f"{part_hash}  {target.name}\n")
    print(f"Verified {len(chunks)} chunks; reassembled SHA256 matches.", flush=True)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
