"""A stand-in for a ComfyUI server, for testing enhance.py with no GPU.

It implements the endpoints the runner uses, and validates every submitted
graph against node_specs.json (transcribed from the ComfyUI source), the
uploaded input files and the installed model lists - much as the real
server's /prompt validation does - so a wrong node name, input name, link
or model fails here the same way.
"""

import json
import struct
import threading
import urllib.parse
import uuid
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import enhance


def make_png(width, height, alpha):
    """A tiny valid PNG, RGB or RGBA."""
    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))
    channels = 4 if alpha else 3
    row = b"\x00" + bytes([120, 90, 60, 255][:channels]) * width
    header = struct.pack(">IIBBBBB", width, height, 8, 6 if alpha else 2, 0, 0, 0)
    return enhance.PNG_MAGIC + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(row * height)) + chunk(b"IEND", b"")


def make_wav(seconds=0.2, rate=8000):
    """A short, valid, silent WAV file."""
    n = int(seconds * rate)
    data = b"\x00\x00" * n
    return (b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16)
            + b"data" + struct.pack("<I", len(data)) + data)


AUDIO_SAVERS = ("SaveAudio", "SaveAudioMP3")
VOICES = ["Roger (male, american)", "Sarah (female, american)", "Charlie (male, australian)"]


class MockComfy:
    def __init__(self, checkpoints=("sd_xl_base_1.0.safetensors", "stable-audio-open-1.0.safetensors", "ace_step_v1_3.5b.safetensors"),
                 controlnets=("diffusers_xl_canny_full.safetensors",)):
        self.specs = enhance.load_specs()
        self.models = {"checkpoints": list(checkpoints), "controlnet": list(controlnets), "text_encoders": ["t5-base.safetensors"]}
        self.extra = []      # extra_data of every accepted prompt (API keys travel here)
        self.files = {}      # input folder: name -> bytes
        self.prompts = []    # every accepted graph, in order
        self.history = {}
        self.outputs = {}
        self.fail_execution = False
        self.never_finish = False
        self.drop_nodes = set()  # node classes this "old" server lacks
        mock = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def _send(self, status, body, content_type="application/json"):
                data = body if isinstance(body, bytes) else json.dumps(body).encode()
                self.send_response(status)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self):
                url = urllib.parse.urlparse(self.path)
                query = urllib.parse.parse_qs(url.query)
                if url.path == "/system_stats":
                    self._send(200, {"system": {"comfyui_version": "mock"}, "devices": []})
                elif url.path == "/object_info":
                    self._send(200, mock.object_info())
                elif url.path.startswith("/object_info/"):
                    cls = url.path.rsplit("/", 1)[1]
                    info = mock.object_info()
                    self._send(200, {cls: info[cls]} if cls in info else {})
                elif url.path.startswith("/history/"):
                    pid = url.path.rsplit("/", 1)[1]
                    self._send(200, {pid: mock.history[pid]} if pid in mock.history else {})
                elif url.path == "/view":
                    key = (query.get("filename", [""])[0], query.get("subfolder", [""])[0], query.get("type", ["output"])[0])
                    if key in mock.outputs:
                        self._send(200, mock.outputs[key], "image/png")
                    else:
                        self._send(404, b"", "text/plain")
                else:
                    self._send(404, b"", "text/plain")

            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
                if self.path == "/upload/image":
                    fields = mock.parse_multipart(self.headers.get("Content-Type", ""), body)
                    if "image" not in fields or fields.get("overwrite", (None, b""))[1] != b"true":
                        self._send(400, b"", "text/plain")
                        return
                    name, content = fields["image"]
                    mock.files[name] = content
                    self._send(200, {"name": name, "subfolder": "", "type": "input"})
                elif self.path == "/prompt":
                    status, answer = mock.submit(json.loads(body))
                    self._send(status, answer)
                else:
                    self._send(404, b"", "text/plain")

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = "http://127.0.0.1:%d" % self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.httpd.shutdown()
        self.httpd.server_close()

    @staticmethod
    def parse_multipart(content_type, body):
        """{field: (filename or None, bytes)} from a multipart/form-data body."""
        boundary = content_type.split("boundary=", 1)[1].encode()
        fields = {}
        for part in body.split(b"--" + boundary):
            if b"\r\n\r\n" not in part:
                continue
            head, data = part.split(b"\r\n\r\n", 1)
            assert data.endswith(b"\r\n"), "multipart part must end with CRLF"
            disposition = [h for h in head.decode().split("\r\n") if h.lower().startswith("content-disposition")][0]
            params = dict(p.strip().split("=", 1) for p in disposition.split(";")[1:])
            fields[params["name"].strip('"')] = (params.get("filename", "").strip('"') or None, data[:-2])
        return fields

    def object_info(self):
        info = {}
        for cls, spec in self.specs.items():
            if cls in self.drop_nodes:
                continue
            required, optional = {}, {}
            for name, i in spec["inputs"].items():
                if i["type"] == "COMBO":
                    options = (self.models[i["models"]] if i.get("models") else sorted(self.files) if i.get("uploaded")
                               else VOICES if cls == "ElevenLabsVoiceSelector" else i.get("options", []))
                    # real servers use both shapes, depending on the node's API version
                    entry = ["COMBO", {"options": options}] if cls == "KSampler" else [options, {}]
                else:
                    entry = [i["type"], {}]
                (optional if i.get("optional") else required)[name] = entry
            info[cls] = {"input": {"required": required, "optional": optional}, "output": spec["outputs"]}
        return info

    def submit(self, payload):
        prompt = payload.get("prompt")
        if not isinstance(prompt, dict) or "client_id" not in payload:
            return 400, {"error": {"type": "no_prompt", "message": "No prompt provided", "details": ""}, "node_errors": {}}
        errors = enhance.validate_graph(prompt, self.specs, self.models, set(self.files))
        saves = [nid for nid, node in prompt.items() if node.get("class_type") in ("SaveImage",) + AUDIO_SAVERS]
        for node in prompt.values():
            if node.get("class_type") == "ElevenLabsVoiceSelector" and node["inputs"].get("voice") not in VOICES:
                errors.append("voice %r is not one of the stock voices" % node["inputs"].get("voice"))
        if not saves:
            errors.append("prompt has no outputs")
        if errors:
            node_errors = {"0": {"class_type": "?", "errors": [{"message": e, "details": ""} for e in errors]}}
            return 400, {"error": {"type": "prompt_outputs_failed_validation", "message": "Prompt outputs failed validation", "details": ""}, "node_errors": node_errors}
        pid = str(uuid.uuid4())
        self.prompts.append(prompt)
        self.extra.append(payload.get("extra_data", {}))
        if self.never_finish:
            return 200, {"prompt_id": pid, "number": len(self.prompts), "node_errors": {}}
        if self.fail_execution:
            messages = [["execution_error", {"node_id": "10", "node_type": "KSampler", "exception_message": "CUDA out of memory"}]]
            self.history[pid] = {"outputs": {}, "status": {"status_str": "error", "completed": False, "messages": messages}}
        else:
            outputs = {}
            for nid in saves:
                prefix = prompt[nid]["inputs"]["filename_prefix"]
                subfolder, _, stem = prefix.rpartition("/")
                if prompt[nid]["class_type"] in AUDIO_SAVERS:
                    # the real nodes write FLAC or MP3; a WAV keeps this stand-in honest about what it returns
                    name = stem + "_00001_.wav"
                    self.outputs[(name, subfolder, "output")] = make_wav()
                    outputs[nid] = {"audio": [{"filename": name, "subfolder": subfolder, "type": "output"}]}
                    continue
                source = prompt[prompt[nid]["inputs"]["images"][0]]
                name = stem + "_00001_.png"
                self.outputs[(name, subfolder, "output")] = make_png(8, 8, source["class_type"] == "JoinImageWithAlpha")
                outputs[nid] = {"images": [{"filename": name, "subfolder": subfolder, "type": "output"}]}
            self.history[pid] = {"outputs": outputs, "status": {"status_str": "success", "completed": True, "messages": []}}
        return 200, {"prompt_id": pid, "number": len(self.prompts), "node_errors": {}}
