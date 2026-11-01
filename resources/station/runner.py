#!/usr/bin/env python3
"""Small, dependency-free render station runner with safe fallback artifacts."""

import argparse
import json
import struct
import sys
import time
import wave
import zlib
from pathlib import Path


def log(message):
    print(f"[station] {message}", flush=True)


def png_bytes(prompt):
    # A 1x1 RGBA PNG is enough to give downstream tools a real image artifact.
    color = zlib.crc32(prompt.encode("utf-8")) & 0xFFFFFF
    pixel = bytes((color >> 16, (color >> 8) & 0xFF, color & 0xFF, 255))

    def chunk(kind, payload):
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"\x00" + pixel)) + chunk(b"IEND", b"")


def write_image(output, prompt):
    output.write_bytes(png_bytes(prompt))


def write_video_fallback(output, prompt, params):
    # A readable sidecar-style artifact is preferable to claiming a fake MP4.
    output.write_text(json.dumps({"fallback": True, "prompt": prompt, "params": params}, indent=2) + "\n", encoding="utf-8")


def write_audio(output, params):
    duration = max(1, min(5, int(params.get("duration", 1))))
    with wave.open(str(output), "wb") as audio:
        audio.setnchannels(1)
        audio.setsampwidth(2)
        audio.setframerate(8000)
        audio.writeframes(b"\x00\x00" * 8000 * duration)


def main():
    parser = argparse.ArgumentParser(description="Run a local Engram render station job.")
    parser.add_argument("--type", choices=("image", "video", "audio"), required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--params", default="{}")
    args = parser.parse_args()

    try:
        if not args.prompt.strip():
            raise ValueError("--prompt must not be empty")
        params = json.loads(args.params)
        if not isinstance(params, dict):
            raise ValueError("--params must be a JSON object")
        output = Path(args.output).expanduser()
        output.parent.mkdir(parents=True, exist_ok=True)
        log(f"Starting {args.type} generation...")
        time.sleep(0.02)
        if args.type == "image":
            write_image(output, args.prompt)
        elif args.type == "audio":
            write_audio(output, params)
        else:
            write_video_fallback(output, args.prompt, params)
            log("No video backend configured; wrote a prompt manifest fallback.")
        log(f"Completed {args.type} generation: {output}")
        return 0
    except Exception as error:
        print(f"[station] ERROR: {error}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
