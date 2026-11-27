#!/usr/bin/env python3
"""Run the game's painted artwork through ComfyUI to enhance it.

The game paints every background and character in code. `export` writes
those paintings out as guide images; `run` sends each one through a
ComfyUI workflow (img2img + ControlNet, so the result keeps the guide's
composition) and saves the result where the game looks for enhanced art.

Standard library only.  python3 enhance.py --help
"""

import argparse
import fnmatch
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


class EnhanceError(Exception):
    """A problem worth reporting to the user without a traceback."""


# --------------------------------------------------------------- settings


def load_json(path, what):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except FileNotFoundError:
        raise EnhanceError(f"{what} not found: {path}")
    except json.JSONDecodeError as e:
        raise EnhanceError(f"{what} is not valid JSON ({path}): {e}")


def load_manifest(guides):
    path = os.path.join(guides, "manifest.json")
    if not os.path.isfile(path):
        raise EnhanceError(
            f"no manifest at {path}\n  export the guides first:  python3 {os.path.relpath(__file__)} export"
        )
    manifest = load_json(path, "manifest")
    assets = manifest.get("assets")
    if not isinstance(assets, list) or not assets:
        raise EnhanceError(f"manifest has no assets: {path}")
    return assets


def load_specs():
    return load_json(os.path.join(HERE, "node_specs.json"), "node specs")["nodes"]


def asset_kind(asset):
    """Which workflow an asset takes: background, sprite or expression."""
    if asset["kind"] == "background":
        return "background"
    return "expression" if asset.get("base") else "sprite"


def settings_for(asset, cfg):
    """Merge global, per-kind and per-asset settings for one asset."""
    kinds = cfg.get("kinds", {})
    kind = asset_kind(asset)
    merged = {k: v for k, v in cfg.items() if k not in ("kinds", "overrides")}
    if kind == "expression":
        merged.update(kinds.get("sprite", {}))
    merged.update(kinds.get(kind, {}))
    for pattern, extra in cfg.get("overrides", {}).items():
        if fnmatch.fnmatchcase(asset["id"], pattern):
            merged.update(extra)
    if "workflow" not in merged:
        raise EnhanceError(f"config has no workflow for kind '{kind}'")
    return merged


def explicit_keys(asset, cfg):
    """Settings the user set for this asset specifically (per-asset overrides)."""
    keys = set()
    for pattern, extra in cfg.get("overrides", {}).items():
        if fnmatch.fnmatchcase(asset["id"], pattern):
            keys.update(extra)
    return keys


def select(assets, only=None, kind=None):
    """Filter assets and order them so neutral sprites precede expressions."""
    chosen = []
    for a in assets:
        if kind and asset_kind(a) != kind and a["kind"] != kind:
            continue
        if only and not any(fnmatch.fnmatchcase(a["id"], pat) for pat in only):
            continue
        chosen.append(a)
    order = {"background": 0, "sprite": 1, "expression": 2}
    return sorted(chosen, key=lambda a: order[asset_kind(a)])


def seed_for(asset, s):
    # one seed per character, so every expression shares its neutral's noise
    key = asset.get("character") or asset["id"]
    return (int(s.get("seed", 0)) + zlib.crc32(key.encode()) % 100000) % (2**63)


def positive_prompt(asset, s):
    parts = [asset.get("prompt", "")]
    if asset.get("expression_prompt"):
        parts.append(asset["expression_prompt"])
    return s.get("style_prefix", "") + ", ".join(p for p in parts if p) + s.get("style_suffix", "")


# -------------------------------------------------------------- workflows


def fill(node, values):
    """Replace every "$name" string in a template with its value."""
    if isinstance(node, dict):
        return {k: fill(v, values) for k, v in node.items()}
    if isinstance(node, list):
        return [fill(v, values) for v in node]
    if isinstance(node, str) and node.startswith("$"):
        if node[1:] not in values:
            raise EnhanceError(f"workflow needs a value for '{node[1:]}' (set it in the config)")
        return values[node[1:]]
    return node


def drop_canny(prompt):
    """Feed the guide straight to the ControlNet (for tile/depth-style models)."""
    for nid in [n for n, node in prompt.items() if node["class_type"] == "Canny"]:
        source = prompt[nid]["inputs"]["image"]
        del prompt[nid]
        for node in prompt.values():
            for name, value in node["inputs"].items():
                if isinstance(value, list) and len(value) == 2 and value[0] == nid:
                    node["inputs"][name] = source
    return prompt


def input_name(asset, role):
    """File name an image is uploaded under in ComfyUI's input folder."""
    if role == "base":
        return "enhanced_" + asset["base"] + ".png"
    return asset[role]


def build_prompt(asset, cfg):
    """Return (prompt graph, output node id, {upload name: role}) for an asset."""
    s = settings_for(asset, cfg)
    template = load_json(os.path.join(HERE, "workflows", s["workflow"]), "workflow")
    # a workflow may carry defaults of its own; explicit settings still win
    for key, value in template.get("defaults", {}).items():
        if key not in explicit_keys(asset, cfg):
            s[key] = value
    values = dict(s)
    values.update(
        positive=positive_prompt(asset, s),
        seed=seed_for(asset, s),
        prefix="below_stairs/" + asset["id"],
    )
    uploads = {}
    kind = asset_kind(asset)
    roles = {"background": {"guide": "file"}, "sprite": {"guide": "flat", "silhouette": "mask"}}.get(
        kind, {"guide": "flat", "silhouette": "mask", "face": "face_mask", "base": "base"}
    )
    needed = json.dumps(template["prompt"])
    for var, role in roles.items():
        if f'"${var}"' not in needed:
            continue  # this workflow does not use that image
        if not asset.get(role):
            raise EnhanceError(f"{asset['id']}: manifest entry has no '{role}' (re-export the guides)")
        values[var] = input_name(asset, role)
        uploads[values[var]] = role
    if template.get("guide") == "enhanced" and "guide" in values:
        # start from the current enhanced image when there is one
        del uploads[values["guide"]]
        values["guide"] = "current_" + asset["output"]
        uploads[values["guide"]] = "current"
    prompt = fill(template["prompt"], values)
    if s.get("control_preprocess", "canny") == "none":
        prompt = drop_canny(prompt)
    elif s.get("control_preprocess", "canny") != "canny":
        raise EnhanceError("control_preprocess must be 'canny' or 'none'")
    return prompt, template["output_node"], uploads


def validate_graph(prompt, specs, models=None, files=None):
    """Check a prompt graph against the node signatures. Returns error strings.

    `models` maps a model folder (checkpoints, controlnet) to the names the
    server has; `files` is the set of images present in its input folder.
    Either may be None to skip that check.
    """
    errors = []
    for nid, node in prompt.items():
        cls = node.get("class_type")
        where = f"node {nid} ({cls})"
        if cls not in specs:
            errors.append(f"{where}: unknown node class")
            continue
        spec = specs[cls]["inputs"]
        given = node.get("inputs", {})
        for name in given:
            if name not in spec:
                errors.append(f"{where}: unexpected input '{name}'")
        for name, info in spec.items():
            if name not in given:
                if not info.get("optional"):
                    errors.append(f"{where}: missing input '{name}'")
                continue
            value, kind = given[name], info["type"]
            if isinstance(value, list):
                # a link: [source node id, output index]
                if len(value) != 2 or value[0] not in prompt:
                    errors.append(f"{where}: input '{name}' links to a missing node")
                    continue
                source = specs.get(prompt[value[0]].get("class_type"))
                if source is None:
                    continue
                if not isinstance(value[1], int) or not 0 <= value[1] < len(source["outputs"]):
                    errors.append(f"{where}: input '{name}' uses output {value[1]} of node {value[0]}, which does not exist")
                elif source["outputs"][value[1]] != kind:
                    errors.append(f"{where}: input '{name}' wants {kind} but node {value[0]} output {value[1]} is {source['outputs'][value[1]]}")
                continue
            if kind == "INT" and (isinstance(value, bool) or not isinstance(value, int)):
                errors.append(f"{where}: '{name}' must be an integer, got {value!r}")
            elif kind == "FLOAT" and (isinstance(value, bool) or not isinstance(value, (int, float))):
                errors.append(f"{where}: '{name}' must be a number, got {value!r}")
            elif kind == "BOOLEAN" and not isinstance(value, bool):
                errors.append(f"{where}: '{name}' must be true or false, got {value!r}")
            elif kind in ("STRING", "COMBO") and not isinstance(value, str):
                errors.append(f"{where}: '{name}' must be text, got {value!r}")
            elif kind in ("INT", "FLOAT"):
                if "min" in info and value < info["min"] or "max" in info and value > info["max"]:
                    errors.append(f"{where}: '{name}' = {value} is outside {info.get('min')}..{info.get('max')}")
            elif kind == "COMBO":
                if "options" in info and value not in info["options"]:
                    errors.append(f"{where}: '{name}' = {value!r} is not one of {info['options']}")
                if info.get("models") and models is not None and value not in models.get(info["models"], []):
                    errors.append(f"{where}: model '{value}' is not in the server's {info['models']} folder")
                if info.get("uploaded") and files is not None and value not in files:
                    errors.append(f"{where}: image '{value}' is not in the server's input folder")
            elif kind not in ("STRING", "BOOLEAN"):
                errors.append(f"{where}: input '{name}' must be a link to a {kind} output")
    return errors


# ------------------------------------------------------------ HTTP client


class Comfy:
    def __init__(self, server, timeout=30):
        self.server = server.rstrip("/")
        self.timeout = timeout
        self.client_id = uuid.uuid4().hex

    def _open(self, path, data=None, headers=None):
        req = urllib.request.Request(self.server + path, data=data, headers=headers or {})
        try:
            return urllib.request.urlopen(req, timeout=self.timeout)
        except urllib.error.HTTPError as e:
            body = e.read().decode("utf-8", "replace")
            raise EnhanceError(f"ComfyUI answered {e.code} for {path}: {describe_error(body)}")
        except (urllib.error.URLError, OSError) as e:
            reason = getattr(e, "reason", e)
            raise ConnectionError(f"cannot reach ComfyUI at {self.server} ({reason})")

    def get_json(self, path):
        with self._open(path) as r:
            return json.loads(r.read().decode("utf-8"))

    def upload(self, path, name):
        """Put a file in ComfyUI's input folder under `name`, replacing any old copy."""
        boundary = "----belowstairs" + uuid.uuid4().hex
        with open(path, "rb") as f:
            content = f.read()
        body = b""
        for field, value in (("overwrite", "true"), ("type", "input")):
            body += f'--{boundary}\r\nContent-Disposition: form-data; name="{field}"\r\n\r\n{value}\r\n'.encode()
        body += (
            f'--{boundary}\r\nContent-Disposition: form-data; name="image"; filename="{name}"\r\n'
            "Content-Type: image/png\r\n\r\n"
        ).encode()
        body += content + f"\r\n--{boundary}--\r\n".encode()
        headers = {"Content-Type": f"multipart/form-data; boundary={boundary}"}
        with self._open("/upload/image", body, headers) as r:
            stored = json.loads(r.read().decode("utf-8")).get("name")
        if stored != name:
            raise EnhanceError(f"ComfyUI stored the upload as '{stored}', expected '{name}'")

    def queue(self, prompt):
        body = json.dumps({"prompt": prompt, "client_id": self.client_id}).encode()
        with self._open("/prompt", body, {"Content-Type": "application/json"}) as r:
            answer = json.loads(r.read().decode("utf-8"))
        if answer.get("node_errors"):
            raise EnhanceError("ComfyUI rejected the workflow: " + describe_error(json.dumps(answer)))
        return answer["prompt_id"]

    def wait(self, prompt_id, output_node, timeout, poll=1.0):
        """Block until a queued prompt finishes; return its first output image record."""
        deadline = time.monotonic() + timeout
        while True:
            entry = self.get_json("/history/" + prompt_id).get(prompt_id)
            if entry:
                status = entry.get("status", {})
                if status.get("status_str") == "error":
                    raise EnhanceError("ComfyUI failed while running the workflow: " + describe_messages(status))
                images = entry.get("outputs", {}).get(output_node, {}).get("images")
                if images:
                    return images[0]
                if status.get("completed"):
                    raise EnhanceError(f"the workflow finished without an image from node {output_node}")
            if time.monotonic() > deadline:
                raise EnhanceError(f"timed out after {timeout:.0f}s waiting for ComfyUI (raise --timeout?)")
            time.sleep(poll)

    def download(self, image):
        query = urllib.parse.urlencode(
            {"filename": image["filename"], "subfolder": image.get("subfolder", ""), "type": image.get("type", "output")}
        )
        with self._open("/view?" + query) as r:
            return r.read()


def describe_error(body):
    """Turn ComfyUI's error JSON into one readable line."""
    try:
        data = json.loads(body)
    except ValueError:
        return body.strip()[:300] or "(no details)"
    parts = []
    err = data.get("error")
    if isinstance(err, dict):
        parts.append(" ".join(str(err.get(k, "")) for k in ("message", "details")).strip())
    elif err:
        parts.append(str(err))
    for nid, info in (data.get("node_errors") or {}).items():
        for e in info.get("errors", []):
            parts.append(f"node {nid} ({info.get('class_type', '?')}): {e.get('message', '')} {e.get('details', '')}".strip())
    return "; ".join(p for p in parts if p) or body.strip()[:300]


def describe_messages(status):
    for name, data in status.get("messages", []):
        if name == "execution_error" and isinstance(data, dict):
            return f"node {data.get('node_id')} ({data.get('node_type')}): {data.get('exception_message', '').strip()}"
    return "no details given"


def combo_options(entry):
    """Options of a combo input from /object_info, old or new format."""
    if isinstance(entry, list) and entry:
        if isinstance(entry[0], list):
            return entry[0]
        if len(entry) > 1 and isinstance(entry[1], dict) and isinstance(entry[1].get("options"), list):
            return entry[1]["options"]
    return None


# --------------------------------------------------------------- commands


def cmd_export(args):
    cmd = ["cargo", "run", "--release", "--", "--export-guides", os.path.abspath(args.guides)]
    print("running:", " ".join(cmd))
    try:
        subprocess.run(cmd, cwd=REPO, check=True)
    except FileNotFoundError:
        raise EnhanceError("cargo was not found; install Rust or export the guides on another machine")
    except subprocess.CalledProcessError as e:
        raise EnhanceError(f"the game failed to export its guides (exit code {e.returncode})")


def cmd_list(args):
    assets = select(load_manifest(args.guides), args.only, args.kind)
    for a in assets:
        done = os.path.isfile(os.path.join(args.out, a["output"]))
        print(f"{'done   ' if done else 'pending'}  {asset_kind(a):<10}  {a['id']}")
    done = sum(os.path.isfile(os.path.join(args.out, a["output"])) for a in assets)
    print(f"{done} of {len(assets)} enhanced in {args.out}")


def cmd_check(args):
    cfg = load_json(args.config, "config")
    specs = load_specs()
    assets = load_manifest(args.guides)
    comfy = Comfy(args.server or cfg.get("server", "http://127.0.0.1:8188"))
    comfy.get_json("/system_stats")
    print(f"ComfyUI is reachable at {comfy.server}")
    info = comfy.get_json("/object_info")
    problems = []
    used = {}
    for a in assets:
        prompt, _, _ = build_prompt(a, cfg)
        problems += [f"{a['id']}: {e}" for e in validate_graph(prompt, specs)]
        for node in prompt.values():
            used.setdefault(node["class_type"], node["inputs"])
    for cls, inputs in sorted(used.items()):
        if cls not in info:
            problems.append(f"this ComfyUI has no node '{cls}' (update ComfyUI)")
            continue
        server_in = info[cls].get("input", {})
        required, optional = server_in.get("required", {}), server_in.get("optional", {})
        for name in required:
            if name not in inputs:
                problems.append(f"node '{cls}' now requires input '{name}', which the workflows do not set")
        for name, value in inputs.items():
            if name not in required and name not in optional:
                problems.append(f"node '{cls}' no longer has input '{name}'")
                continue
            options = combo_options(required.get(name, optional.get(name)))
            if options is not None and isinstance(value, str) and value not in options:
                hint = "put the file in ComfyUI's models folder or edit config.json" if specs[cls]["inputs"][name].get("models") else "edit config.json"
                if not specs[cls]["inputs"][name].get("uploaded"):
                    problems.append(f"'{value}' is not available for {cls}.{name} on this server ({hint})")
    problems = sorted(set(problems))
    for p in problems:
        print("PROBLEM:", p)
    if problems:
        raise EnhanceError(f"{len(problems)} problem(s) found")
    print(f"all {len(used)} node types and the configured models are available; {len(assets)} assets ready")


def check_png(data, want_alpha, asset_id):
    if not data.startswith(PNG_MAGIC) or len(data) < 33:
        raise EnhanceError(f"{asset_id}: ComfyUI returned something that is not a PNG")
    if want_alpha and data[25] not in (4, 6):
        raise EnhanceError(f"{asset_id}: the result has no alpha channel; a sprite must be transparent")


def cmd_run(args):
    cfg = load_json(args.config, "config")
    specs = load_specs()
    assets = select(load_manifest(args.guides), args.only, args.kind)
    if not assets:
        raise EnhanceError("no assets match")
    if args.workflow:
        # use one workflow for everything selected, whatever the config says
        kind = load_json(os.path.join(HERE, "workflows", args.workflow), "workflow").get("kind")
        wrong = [a["id"] for a in assets if kind and asset_kind(a) != kind]
        if wrong:
            raise EnhanceError(f"{args.workflow} is a {kind} workflow; narrow the selection (--kind {kind}). Not suitable for: {', '.join(wrong[:4])}")
        cfg = dict(cfg, kinds={k: dict(v, workflow=args.workflow) for k, v in cfg.get("kinds", {}).items()})
    by_id = {a["id"]: a for a in load_manifest(args.guides)}

    if args.dry_run:
        os.makedirs(args.dry_run, exist_ok=True)
        for a in assets:
            prompt, _, _ = build_prompt(a, cfg)
            errors = validate_graph(prompt, specs)
            if errors:
                raise EnhanceError(f"{a['id']}: " + "; ".join(errors))
            with open(os.path.join(args.dry_run, a["id"] + ".json"), "w", encoding="utf-8") as f:
                json.dump(prompt, f, indent=2)
        print(f"wrote {len(assets)} resolved workflows to {args.dry_run} (nothing was sent to ComfyUI)")
        return

    comfy = Comfy(args.server or cfg.get("server", "http://127.0.0.1:8188"))
    done, skipped, failed = 0, 0, []
    for n, a in enumerate(assets, 1):
        target = os.path.join(args.out, a["output"])
        label = f"[{n}/{len(assets)}] {a['id']}"
        if os.path.isfile(target) and not args.force:
            print(f"{label}: already enhanced, skipping (use --force to redo)")
            skipped += 1
            continue
        try:
            prompt, output_node, uploads = build_prompt(a, cfg)
            errors = validate_graph(prompt, specs)
            if errors:
                raise EnhanceError("; ".join(errors))
            for name, role in uploads.items():
                if role == "current":
                    source = target if os.path.isfile(target) else os.path.join(args.guides, a.get("flat") or a["file"])
                elif role == "base":
                    source = os.path.join(args.out, by_id[a["base"]]["output"])
                    if not os.path.isfile(source):
                        raise EnhanceError(f"needs the enhanced neutral portrait first ({a['base']}); run that asset, then this one")
                else:
                    source = os.path.join(args.guides, a[role])
                    if not os.path.isfile(source):
                        raise EnhanceError(f"guide image missing: {source} (re-export the guides)")
                comfy.upload(source, name)
            started = time.monotonic()
            image = comfy.wait(comfy.queue(prompt), output_node, args.timeout, args.poll)
            data = comfy.download(image)
            check_png(data, a["kind"] == "sprite", a["id"])
            os.makedirs(args.out, exist_ok=True)
            tmp = target + ".part"
            with open(tmp, "wb") as f:
                f.write(data)
            os.replace(tmp, target)
            print(f"{label}: saved {target} ({time.monotonic() - started:.0f}s)")
            done += 1
        except ConnectionError as e:
            raise EnhanceError(str(e) + "\n  start ComfyUI, or pass --server URL")
        except EnhanceError as e:
            print(f"{label}: FAILED - {e}", file=sys.stderr)
            failed.append(a["id"])
            if args.fail_fast:
                break
    print(f"{done} enhanced, {skipped} skipped, {len(failed)} failed")
    if failed:
        raise EnhanceError("failed: " + ", ".join(failed) + "\n  fix the cause and run again; finished assets are kept")


def cmd_verify_specs(args):
    """Cross-check node_specs.json against a ComfyUI source checkout."""
    import re

    sources = {}
    for root in (args.comfy_src, os.path.join(args.comfy_src, "comfy_extras"), os.path.join(args.comfy_src, "comfy_api_nodes")):
        if not os.path.isdir(root):
            raise EnhanceError(f"not a ComfyUI checkout: {args.comfy_src}")
        for name in os.listdir(root):
            if name.endswith(".py"):
                with open(os.path.join(root, name), encoding="utf-8") as f:
                    sources[name] = f.read()
    problems = []
    skipped = 0
    for cls, spec in load_specs().items():
        if spec.get("custom"):
            skipped += 1  # a custom node pack, not part of ComfyUI: see its "custom" note for where it was read
            continue
        body = None
        for name, text in sources.items():
            m = re.search(r'node_id="%s"' % re.escape(cls), text) or re.search(r"^class %s\b" % re.escape(cls), text, re.M)
            if m:
                # the definition runs until the next class
                end = text.find("\nclass ", m.end())
                body = text[m.start() : end if end > 0 else len(text)]
                break
        if body is None:
            problems.append(f"{cls}: no definition found")
            continue
        for inp in spec["inputs"]:
            # dynamic inputs are written "group.name" (and "group.prefixN" for growing lists)
            for part in inp.split("."):
                if f'"{part}"' not in body and f'"{part.rstrip("0123456789")}"' not in body and not any(f'"{part}"' in t for t in sources.values()):
                    problems.append(f"{cls}: input '{inp}' not found in its definition")
        for opt in [o for info in spec["inputs"].values() if not info.get("server_list") for o in info.get("options", [])]:
            if f'"{opt}"' not in body and f"'{opt}'" not in body and not any(f'"{opt}"' in t for t in sources.values()):
                problems.append(f"{cls}: option '{opt}' not found")
    for p in problems:
        print("MISMATCH:", p)
    if problems:
        raise EnhanceError(f"{len(problems)} mismatch(es) between node_specs.json and {args.comfy_src}")
    print(f"node_specs.json matches {args.comfy_src} ({len(load_specs()) - skipped} node classes checked, {skipped} custom skipped)")


def cmd_sheet(args):
    """Before/after sheets for review (needs Pillow)."""
    try:
        from PIL import Image
    except ImportError:
        raise EnhanceError("the sheet command needs Pillow:  python3 -m pip install pillow")
    assets = select(load_manifest(args.guides), args.only, args.kind)
    os.makedirs(args.review, exist_ok=True)
    made = 0
    for a in assets:
        after_path = os.path.join(args.out, a["output"])
        if not os.path.isfile(after_path):
            continue
        before = Image.open(os.path.join(args.guides, a["file"])).convert("RGBA")
        after = Image.open(after_path).convert("RGBA").resize(before.size, Image.LANCZOS)
        sheet = Image.new("RGBA", (before.width * 2 + 12, before.height), (96, 100, 120, 255))
        sheet.alpha_composite(before, (0, 0))
        sheet.alpha_composite(after, (before.width + 12, 0))
        sheet.convert("RGB").save(os.path.join(args.review, a["id"] + ".png"))
        made += 1
    print(f"wrote {made} before/after sheets to {args.review}")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--config", default=os.path.join(HERE, "config.json"), help="settings file (default: comfyui/config.json)")
    p.add_argument("--guides", default=os.path.join(HERE, "guides"), help="exported guide images (default: comfyui/guides)")
    p.add_argument("--out", default=os.path.join(REPO, "assets", "enhanced"), help="where enhanced art goes (default: assets/enhanced)")
    p.add_argument("--server", help="ComfyUI address, overriding the config (e.g. http://127.0.0.1:8188)")
    sub = p.add_subparsers(dest="command", required=True)

    def filters(sp):
        sp.add_argument("--only", action="append", metavar="GLOB", help="asset ids to include, e.g. 'bg_*' or 'spr_q_*' (repeatable)")
        sp.add_argument("--kind", choices=["background", "sprite", "expression"], help="only this kind ('sprite' = neutral portraits and expressions)")

    sub.add_parser("export", help="have the game write its guide images").set_defaults(fn=cmd_export)
    sp = sub.add_parser("list", help="show assets and which are already enhanced")
    filters(sp)
    sp.set_defaults(fn=cmd_list)
    sub.add_parser("check", help="verify the ComfyUI server, nodes and models").set_defaults(fn=cmd_check)
    sp = sub.add_parser("run", help="enhance assets through ComfyUI")
    filters(sp)
    sp.add_argument("--force", action="store_true", help="redo assets that already have an enhanced image")
    sp.add_argument("--workflow", metavar="FILE", help="use this workflow from comfyui/workflows for every selected asset, e.g. background_reimagine.json")
    sp.add_argument("--dry-run", metavar="DIR", help="write the resolved workflow for each asset to DIR; contact no server")
    sp.add_argument("--timeout", type=float, default=1800, help="seconds to wait for one image (default 1800)")
    sp.add_argument("--poll", type=float, default=1.0, help=argparse.SUPPRESS)
    sp.add_argument("--fail-fast", action="store_true", help="stop at the first failure")
    sp.set_defaults(fn=cmd_run)
    sp = sub.add_parser("sheet", help="write before/after comparison images (needs Pillow)")
    filters(sp)
    sp.add_argument("--review", default=os.path.join(HERE, "review"), help="output folder (default: comfyui/review)")
    sp.set_defaults(fn=cmd_sheet)
    sp = sub.add_parser("verify-specs", help="check node_specs.json against a ComfyUI source checkout")
    sp.add_argument("--comfy-src", required=True, metavar="DIR")
    sp.set_defaults(fn=cmd_verify_specs)

    args = p.parse_args(argv)
    try:
        args.fn(args)
    except ConnectionError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    except EnhanceError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
