#!/usr/bin/env python3
"""End-to-end checks for the dependency-free render station fallback runner."""

import binascii
import json
import shutil
import struct
import subprocess
import sys
import tempfile
import wave
from pathlib import Path


ROOT = Path(__file__).resolve().parent
RUNNER = ROOT / "runner.py"
OUT = Path(tempfile.gettempdir()) / "station_test"


def run(*args):
    return subprocess.run(
        [sys.executable, str(RUNNER), *args],
        text=True,
        capture_output=True,
    )


def assert_png(path):
    data = path.read_bytes()
    assert len(data) > 50, f"PNG is unexpectedly small: {len(data)} bytes"
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    offset = 8
    chunks = []
    while offset < len(data):
        assert offset + 12 <= len(data), "truncated PNG chunk"
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        crc = struct.unpack(">I", data[offset + 8 + length : offset + 12 + length])[0]
        assert len(payload) == length, "truncated PNG payload"
        assert binascii.crc32(kind + payload) & 0xFFFFFFFF == crc, f"bad CRC for {kind!r}"
        chunks.append(kind)
        offset += 12 + length
        if kind == b"IEND":
            break
    assert b"IHDR" in chunks
    assert b"IEND" in chunks


def main():
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir(parents=True)

    image = OUT / "mars.png"
    result = run("--type", "image", "--prompt", "A retro sci-fi rover on Mars", "--output", str(image), "--params", '{"seed": 42}')
    assert result.returncode == 0, result.stderr
    assert_png(image)

    audio = OUT / "synth.wav"
    result = run("--type", "audio", "--prompt", "Ambient synth pad", "--output", str(audio), "--params", '{"duration": 3}')
    assert result.returncode == 0, result.stderr
    with wave.open(str(audio), "rb") as stream:
        assert stream.getnchannels() == 1
        assert stream.getsampwidth() == 2
        assert stream.getframerate() == 8000
        assert stream.getnframes() == 8000 * 3

    nested = OUT / "deeply" / "nested" / "output" / "shot.png"
    result = run("--type", "image", "--prompt", "nested", "--output", str(nested), "--params", "{}")
    assert result.returncode == 0, result.stderr
    assert nested.is_file()
    assert_png(nested)

    malformed = run("--type", "image", "--prompt", "bad params", "--output", str(OUT / "bad.png"), "--params", '{"bad": json')
    assert malformed.returncode != 0
    assert "[station] ERROR" in malformed.stderr

    invalid_type = subprocess.run(
        [sys.executable, str(RUNNER), "--type", "3d_model", "--prompt", "test", "--output", str(OUT / "invalid"), "--params", "{}"],
        text=True,
        capture_output=True,
    )
    assert invalid_type.returncode != 0

    empty_prompt = run("--type", "image", "--prompt", "", "--output", str(OUT / "empty.png"), "--params", "{}")
    assert empty_prompt.returncode != 0
    assert "[station] ERROR" in empty_prompt.stderr

    print("runner task tests passed")


if __name__ == "__main__":
    main()
