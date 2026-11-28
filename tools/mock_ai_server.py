#!/usr/bin/env python3
"""A test stand-in for the Claude API and for Ollama, as one local server.

    python3 tools/mock_ai_server.py [LOG.json]

Prints the address it is listening on. Point the editor at it with
ANTHROPIC_BASE_URL=<address> (and any ANTHROPIC_API_KEY) and
OLLAMA_HOST=<address>. It answers:

- POST /v1/messages: a streamed answer (server-sent events), as the Claude
  API sends one, holding a made-up character or place that fits the schema
  it was asked for. A brief containing "refuse" gets a refusal; "busy" gets
  one 529 first.
- GET /api/tags and POST /api/chat: as Ollama does.

Every request it is sent (headers too, except the key) is logged to LOG.json.
"""
import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = sys.argv[1] if len(sys.argv) > 1 else None
seen = []
busy_once = {"done": False}
lock = threading.Lock()


def made(system, user, schema):
    """Something that fits what was asked for."""
    props = schema.get("properties", {})
    if "sprite" in props:
        return {
            "id": "mrs_hobb", "name": "Mrs Hobb", "color": "#e0b0ff", "sprite": "c",
            "look": {"skin": "#d8b090", "hair": "#6a6a70", "brow": "#505055", "iris": "#304060", "coat": "#2a2a3a",
                     "width": 58, "jaw": 0.8, "age": 0.8, "blush": 0.1, "shoulders": 110, "lashes": 0.2, "lips": 0.3},
            "bio": "The housekeeper. She keeps every key.",
            "art_prompt": "a stern housekeeper in her fifties, grey hair under a cap, black dress, a ring of keys",
        }
    out = {
        "name": "servants_stair", "title": "The servants' stair", "prompt": "a narrow servants' staircase at night, one candle",
        "vignette": 0.4,
        "layers": [
            {"shape": "gradient", "top": "#10121c", "bottom": "#2a2430", "y0": 0, "y1": 648},
            {"shape": "poly", "points": [{"x": 300, "y": 648}, {"x": 520, "y": 200}, {"x": 640, "y": 200}, {"x": 860, "y": 648}], "color": "#3a3028", "alpha": 1},
            {"shape": "rect", "x": 0, "y": 520, "w": 1152, "h": 128, "color": "#1a1610", "alpha": 1},
            {"shape": "ellipse", "x": 760, "y": 300, "rx": 6, "ry": 14, "color": "#fff0c0", "alpha": 1},
            {"shape": "glow", "x": 760, "y": 300, "r": 220, "color": "#ffb060", "strength": 0.9},
            {"shape": "stroke", "points": [{"x": 300, "y": 600, "p": 0.6}, {"x": 600, "y": 590, "p": 0.9}, {"x": 900, "y": 605, "p": 0.5}], "width": 80, "color": "#4a3a2a", "brush": "flat", "load": 0.3, "alpha": 1, "seed": 4},
            {"shape": "teapot", "x": 1},
        ],
    }
    if "room" in props:
        out["room"] = {
            "sky": "#101018", "ambient": "#806050", "brightness": 200, "half_width": 4, "half_depth": 8,
            "parts": [
                {"shape": "box", "at": {"x": 0, "y": -0.1, "z": 0}, "size": {"x": 8, "y": 0.2, "z": 16}, "turn": 0, "color": "#3a3028", "glow": 0, "solid": False, "name": "floor"},
                {"shape": "box", "at": {"x": -4, "y": 2, "z": 0}, "size": {"x": 0.2, "y": 4, "z": 16}, "turn": 0, "color": "#504438", "glow": 0, "solid": True, "name": "wall"},
            ],
            "lights": [{"kind": "point", "at": {"x": 1, "y": 2, "z": 0}, "color": "#ffb060", "power": 600}],
        }
    return out


def sse(event):
    return ("event: %s\ndata: %s\n\n" % (event["type"], json.dumps(event))).encode()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def note(self, body):
        headers = {k.lower(): v for k, v in self.headers.items() if k.lower() not in ("x-api-key", "authorization")}
        with lock:
            seen.append({"path": self.path, "headers": headers, "body": body, "keyed": bool(self.headers.get("x-api-key") or self.headers.get("authorization"))})
            if LOG:
                with open(LOG + ".tmp", "w") as f:
                    json.dump(seen, f)
                os.replace(LOG + ".tmp", LOG)

    def reply(self, code, obj):
        data = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.note(None)
        if self.path == "/api/tags":
            return self.reply(200, {"models": [{"name": "llama3.3:70b"}, {"name": "qwen3:32b"}]})
        self.reply(404, {"error": "not found"})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))) or b"null")
        self.note(body)
        if self.path == "/api/chat":
            system = body["messages"][0]["content"]
            user = body["messages"][1]["content"]
            return self.reply(200, {"model": body["model"], "done": True, "message": {"role": "assistant", "content": json.dumps(made(system, user, body["format"]))}})
        if self.path != "/v1/messages":
            return self.reply(404, {"error": "not found"})
        if not (self.headers.get("x-api-key") or self.headers.get("authorization")):
            return self.reply(401, {"type": "error", "error": {"type": "authentication_error", "message": "no key"}})
        user = body["messages"][0]["content"]
        if "busy" in user and not busy_once["done"]:
            busy_once["done"] = True
            self.send_response(529)
            self.send_header("retry-after", "1")
            data = json.dumps({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}).encode()
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        self.wfile.write(sse({"type": "message_start", "message": {"id": "msg_1", "type": "message", "role": "assistant", "content": [], "model": body["model"]}}))
        if "refuse" in user:
            self.wfile.write(sse({"type": "message_delta", "delta": {"stop_reason": "refusal"}, "usage": {"output_tokens": 1}}))
            self.wfile.write(sse({"type": "message_stop"}))
            return
        text = json.dumps(made(body.get("system", ""), user, body["output_config"]["format"]["schema"]))
        self.wfile.write(sse({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}))
        self.wfile.write(sse({"type": "ping"}))
        for i in range(0, len(text), 40):
            self.wfile.write(sse({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text[i:i + 40]}}))
            self.wfile.flush()
        self.wfile.write(sse({"type": "content_block_stop", "index": 0}))
        self.wfile.write(sse({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 100}}))
        self.wfile.write(sse({"type": "message_stop"}))


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
print("http://127.0.0.1:%d" % server.server_address[1], flush=True)
server.serve_forever()
