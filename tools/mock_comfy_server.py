#!/usr/bin/env python3
"""Run the test stand-in for ComfyUI as a real server (no GPU, no models).

    python3 tools/mock_comfy_server.py [LOG.json]

Prints the address it is listening on. Every workflow it accepts is
appended to LOG.json, so a test can see what was sent. Useful for trying
the editor's Art view without ComfyUI: point "ComfyUI at" to the address.
"""
import json
import os
import sys
import time

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "comfyui")
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, "tests"))
from mock_comfy import MockComfy  # noqa: E402

mock = MockComfy()
print(mock.url, flush=True)
log, written = (sys.argv[1] if len(sys.argv) > 1 else None), -1
try:
    while True:
        if log and len(mock.prompts) != written:
            written = len(mock.prompts)
            with open(log + ".tmp", "w") as f:
                json.dump({"prompts": mock.prompts, "files": sorted(mock.files)}, f)
            os.replace(log + ".tmp", log)
        time.sleep(0.1)
except KeyboardInterrupt:
    mock.close()
