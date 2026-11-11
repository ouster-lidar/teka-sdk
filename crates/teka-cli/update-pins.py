#!/usr/bin/env python3
"""Refresh direct-Cargo fallback revisions from the repository's flake.lock."""

import json
from pathlib import Path

crate = Path(__file__).resolve().parent
lock = json.loads((crate.parent.parent / "flake.lock").read_text())
pins = {"nixpkgs": lock["nodes"]["nixpkgs"]["locked"]["rev"]}
(crate / "src/fallback-pins.json").write_text(json.dumps(pins, indent=2) + "\n")
