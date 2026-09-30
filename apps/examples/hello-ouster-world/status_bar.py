#!/usr/bin/env python3
"""Show the source status on Sway outputs while SimpleViz is waiting."""

import json
import os
import time
from pathlib import Path

STATUS_PATH = Path(
    os.environ.get("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR", "/run/hello-ouster-world")
) / "source-runtime.json"


def status_text() -> str:
    try:
        status = json.loads(STATUS_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        status = {}
    message = str(status.get("message") or "Starting sensor discovery")
    return "OUSTER  " + " ".join(message.split())


def main() -> None:
    while True:
        print(status_text(), flush=True)
        time.sleep(1)


if __name__ == "__main__":
    main()
