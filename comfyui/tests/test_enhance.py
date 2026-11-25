"""End-to-end tests for the ComfyUI suite. No network, no GPU.

    python3 -m unittest discover comfyui/tests
"""

import contextlib
import copy
import io
import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)

import enhance  # noqa: E402
from mock_comfy import MockComfy, make_png  # noqa: E402

CHARS = ["q", "t"]
EXPRS = ["neutral", "smile", "shock"]


def write_fixture(folder):
    """A small guide export shaped exactly like the game's."""
    assets = []
    for name in ["pantry", "axis"]:
        f = f"bg_{name}.png"
        with open(os.path.join(folder, f), "wb") as out:
            out.write(make_png(8, 8, False))
        assets.append({"id": f"bg_{name}", "kind": "background", "file": f, "output": f, "width": 1152, "height": 648, "prompt": f"the {name}"})
    for who in CHARS:
        with open(os.path.join(folder, f"face_{who}.png"), "wb") as out:
            out.write(make_png(8, 8, False))
        for expr in EXPRS:
            stem = f"{who}_{expr}"
            for prefix, alpha in (("spr", True), ("flat", False), ("mask", False)):
                with open(os.path.join(folder, f"{prefix}_{stem}.png"), "wb") as out:
                    out.write(make_png(8, 8, alpha))
            assets.append({
                "id": f"spr_{stem}", "kind": "sprite", "file": f"spr_{stem}.png", "output": f"spr_{stem}.png",
                "width": 336, "height": 456, "character": who, "expression": expr,
                "base": None if expr == "neutral" else f"spr_{who}_neutral",
                "flat": f"flat_{stem}.png", "mask": f"mask_{stem}.png", "face_mask": f"face_{who}.png",
                "face_rect": [100, 150, 120, 130], "prompt": f"portrait of {who}", "expression_prompt": expr,
            })
    # expressions listed before their neutral, to prove the runner reorders
    assets.sort(key=lambda a: a.get("expression") == "neutral")
    with open(os.path.join(folder, "manifest.json"), "w") as out:
        json.dump({"version": 1, "assets": assets}, out)
    return assets


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.guides = os.path.join(self.tmp.name, "guides")
        self.out = os.path.join(self.tmp.name, "enhanced")
        os.makedirs(self.guides)
        self.assets = write_fixture(self.guides)
        self.cfg = enhance.load_json(os.path.join(os.path.dirname(HERE), "config.json"), "config")
        self.specs = enhance.load_specs()
        self.mock = None

    def tearDown(self):
        if self.mock:
            self.mock.close()
        self.tmp.cleanup()

    def server(self, **kw):
        self.mock = MockComfy(**kw)
        return self.mock

    def run_cli(self, *argv, config=None):
        args = ["--guides", self.guides, "--out", self.out]
        if config is not None:
            path = os.path.join(self.tmp.name, "config.json")
            with open(path, "w") as f:
                json.dump(config, f)
            args += ["--config", path]
        if self.mock:
            args += ["--server", self.mock.url]
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = enhance.main(args + list(argv))
        return code, out.getvalue(), err.getvalue()


class Workflows(Base):
    def test_every_asset_builds_a_valid_graph(self):
        for a in self.assets:
            prompt, output_node, uploads = enhance.build_prompt(a, self.cfg)
            self.assertEqual(enhance.validate_graph(prompt, self.specs), [], a["id"])
            self.assertEqual(prompt[output_node]["class_type"], "SaveImage")
            self.assertNotIn("$", json.dumps(prompt), "unfilled placeholder")
            self.assertTrue(uploads)

    def test_real_export_is_valid_if_present(self):
        guides = os.path.join(os.path.dirname(HERE), "guides")
        if not os.path.isfile(os.path.join(guides, "manifest.json")):
            self.skipTest("no exported guides")
        assets = enhance.load_manifest(guides)
        self.assertEqual(len(assets), 49)
        for a in assets:
            prompt, _, uploads = enhance.build_prompt(a, self.cfg)
            self.assertEqual(enhance.validate_graph(prompt, self.specs), [], a["id"])
            for name, role in uploads.items():
                if role != "base":
                    self.assertTrue(os.path.isfile(os.path.join(guides, a[role])), name)

    def test_generation_sizes_suit_the_vae(self):
        for a in self.assets:
            s = enhance.settings_for(a, self.cfg)
            self.assertEqual(s["width"] % 8, 0)
            self.assertEqual(s["height"] % 8, 0)
            # aspect ratio within 1% of the game's
            self.assertAlmostEqual(s["width"] / s["height"], a["width"] / a["height"], delta=0.01 * a["width"] / a["height"])

    def test_validator_catches_real_mistakes(self):
        good, _, _ = enhance.build_prompt(self.assets[-1], self.cfg)
        def errors_after(change):
            p = copy.deepcopy(good)
            change(p)
            return enhance.validate_graph(p, self.specs)
        self.assertTrue(errors_after(lambda p: p["10"]["inputs"].__setitem__("sampler_name", "eulerr")))
        self.assertTrue(errors_after(lambda p: p["10"]["inputs"].pop("denoise")))
        self.assertTrue(errors_after(lambda p: p["10"]["inputs"].__setitem__("latent", p["10"]["inputs"].pop("latent_image"))))
        self.assertTrue(errors_after(lambda p: p["10"]["inputs"].__setitem__("model", ["1", 1])))   # CLIP into MODEL
        self.assertTrue(errors_after(lambda p: p["11"]["inputs"].__setitem__("samples", ["99", 0])))
        self.assertTrue(errors_after(lambda p: p["3"]["inputs"].__setitem__("width", "1024")))
        self.assertTrue(errors_after(lambda p: p["6"]["inputs"].__setitem__("low_threshold", 0.0)))
        self.assertTrue(errors_after(lambda p: p["1"].__setitem__("class_type", "CheckpointLoader2")))

    def test_sprites_get_alpha_with_the_right_polarity(self):
        # JoinImageWithAlpha treats its mask as 1 = transparent, so a
        # white-is-opaque silhouette must pass through InvertMask first.
        for a in self.assets:
            if a["kind"] != "sprite":
                continue
            prompt, out, _ = enhance.build_prompt(a, self.cfg)
            join = prompt[prompt[out]["inputs"]["images"][0]]
            self.assertEqual(join["class_type"], "JoinImageWithAlpha")
            self.assertEqual(prompt[join["inputs"]["alpha"][0]]["class_type"], "InvertMask")

    def test_expression_repaints_only_the_face_of_the_enhanced_neutral(self):
        a = next(x for x in self.assets if x.get("base"))
        prompt, _, uploads = enhance.build_prompt(a, self.cfg)
        by_class = lambda c: [n for n in prompt.values() if n["class_type"] == c]
        self.assertIn("enhanced_spr_%s_neutral.png" % a["character"], uploads)
        sampler = by_class("KSampler")[0]
        noise_mask = prompt[sampler["inputs"]["latent_image"][0]]
        self.assertEqual(noise_mask["class_type"], "SetLatentNoiseMask")
        # the latent being repainted is the encoded *enhanced neutral*
        encode = prompt[noise_mask["inputs"]["samples"][0]]
        scaled = prompt[encode["inputs"]["pixels"][0]]
        self.assertEqual(prompt[scaled["inputs"]["image"][0]]["inputs"]["image"], "enhanced_" + a["base"] + ".png")
        # and the result is pasted back over that same neutral with the same mask
        comp = by_class("ImageCompositeMasked")[0]
        self.assertEqual(comp["inputs"]["destination"], encode["inputs"]["pixels"])
        self.assertEqual(comp["inputs"]["mask"], noise_mask["inputs"]["mask"])
        # the control image comes from this expression's guide
        control = by_class("ControlNetApplyAdvanced")[0]
        canny = prompt[control["inputs"]["image"][0]]
        guide = prompt[prompt[canny["inputs"]["image"][0]]["inputs"]["image"][0]]
        self.assertEqual(guide["inputs"]["image"], a["flat"])

    def test_expressions_share_their_neutrals_seed(self):
        for who in CHARS:
            seeds = {enhance.seed_for(a, self.cfg) for a in self.assets if a.get("character") == who}
            self.assertEqual(len(seeds), 1)

    def test_canny_can_be_bypassed(self):
        cfg = dict(self.cfg, control_preprocess="none")
        for a in self.assets:
            prompt, _, _ = enhance.build_prompt(a, cfg)
            self.assertFalse([n for n in prompt.values() if n["class_type"] == "Canny"])
            self.assertEqual(enhance.validate_graph(prompt, self.specs), [])

    def test_every_shipped_workflow_is_valid_for_its_kind(self):
        folder = os.path.join(os.path.dirname(HERE), "workflows")
        seen = set()
        for name in sorted(os.listdir(folder)):
            template = enhance.load_json(os.path.join(folder, name), "workflow")
            self.assertIn(template["kind"], ("background", "sprite", "expression"), name)
            self.assertTrue(template["title"])
            cfg = dict(self.cfg, kinds={k: dict(v, workflow=name) for k, v in self.cfg["kinds"].items()})
            for a in self.assets:
                if enhance.asset_kind(a) != template["kind"]:
                    continue
                prompt, out, uploads = enhance.build_prompt(a, cfg)
                self.assertEqual(enhance.validate_graph(prompt, self.specs), [], name)
                self.assertNotIn('"$', json.dumps(prompt), name)
                seen.add(name)
        self.assertEqual(len(seen), 6)

    def test_reimagine_starts_from_noise_and_refine_from_the_current_image(self):
        bg = next(a for a in self.assets if a["id"] == "bg_axis")
        cfg = lambda name: dict(self.cfg, kinds={k: dict(v, workflow=name) for k, v in self.cfg["kinds"].items()})
        prompt, _, _ = enhance.build_prompt(bg, cfg("background_reimagine.json"))
        sampler = next(n for n in prompt.values() if n["class_type"] == "KSampler")
        self.assertEqual(prompt[sampler["inputs"]["latent_image"][0]]["class_type"], "EmptyLatentImage")
        self.assertEqual(sampler["inputs"]["denoise"], 1.0)
        self.assertEqual(next(n for n in prompt.values() if n["class_type"] == "ControlNetApplyAdvanced")["inputs"]["strength"], 0.95)
        prompt, _, uploads = enhance.build_prompt(bg, cfg("background_refine.json"))
        self.assertEqual(uploads, {"current_bg_axis.png": "current"})
        self.assertFalse([n for n in prompt.values() if n["class_type"].startswith("ControlNet")])
        self.assertEqual(next(n for n in prompt.values() if n["class_type"] == "KSampler")["inputs"]["denoise"], 0.3)

    def test_overrides_match_by_glob(self):
        cfg = dict(self.cfg, overrides={"spr_q_*": {"denoise": 0.11}, "bg_axis": {"steps": 7}})
        q = enhance.settings_for(next(a for a in self.assets if a["id"] == "spr_q_smile"), cfg)
        t = enhance.settings_for(next(a for a in self.assets if a["id"] == "spr_t_smile"), cfg)
        self.assertEqual(q["denoise"], 0.11)
        self.assertNotEqual(t["denoise"], 0.11)
        self.assertEqual(enhance.settings_for(self.assets[0] if self.assets[0]["id"] == "bg_axis" else next(a for a in self.assets if a["id"] == "bg_axis"), cfg)["steps"], 7)


class Runs(Base):
    def test_full_run_writes_every_asset(self):
        mock = self.server()
        code, out, err = self.run_cli("run", "--poll", "0.01")
        self.assertEqual(code, 0, err)
        for a in self.assets:
            path = os.path.join(self.out, a["output"])
            self.assertTrue(os.path.isfile(path), a["id"])
            with open(path, "rb") as f:
                data = f.read()
            self.assertTrue(data.startswith(enhance.PNG_MAGIC))
            self.assertEqual(data[25], 6 if a["kind"] == "sprite" else 2, a["id"])  # RGBA sprites, RGB backgrounds
        self.assertEqual(len(mock.prompts), len(self.assets))
        self.assertFalse([f for f in os.listdir(self.out) if f.endswith(".part")])
        # each neutral was generated before any expression that builds on it
        kinds = ["expr" if any(n["class_type"] == "SetLatentNoiseMask" for n in p.values()) else "base" for p in mock.prompts]
        self.assertEqual(kinds, sorted(kinds))
        # the enhanced neutral really was sent back for the expression pass
        with open(os.path.join(self.out, "spr_q_neutral.png"), "rb") as f:
            self.assertEqual(mock.files["enhanced_spr_q_neutral.png"], f.read())

    def test_rerun_skips_finished_work_and_force_redoes_it(self):
        mock = self.server()
        self.run_cli("run", "--poll", "0.01")
        first = len(mock.prompts)
        code, out, _ = self.run_cli("run", "--poll", "0.01")
        self.assertEqual(code, 0)
        self.assertEqual(len(mock.prompts), first)
        self.assertIn("skipping", out)
        self.run_cli("run", "--poll", "0.01", "--force", "--only", "bg_axis")
        self.assertEqual(len(mock.prompts), first + 1)

    def test_workflow_option_runs_another_workflow(self):
        mock = self.server()
        code, _, err = self.run_cli("run", "--poll", "0.01", "--kind", "background", "--workflow", "background_reimagine.json")
        self.assertEqual(code, 0, err)
        self.assertTrue(all(any(n["class_type"] == "EmptyLatentImage" for n in p.values()) for p in mock.prompts))
        # refine picks up the image that run just made
        code, _, err = self.run_cli("run", "--poll", "0.01", "--only", "bg_axis", "--force", "--workflow", "background_refine.json")
        self.assertEqual(code, 0, err)
        with open(os.path.join(self.out, "bg_axis.png"), "rb") as f:
            self.assertTrue(f.read().startswith(enhance.PNG_MAGIC))
        self.assertIn("current_bg_axis.png", mock.files)
        code, _, err = self.run_cli("run", "--workflow", "background_refine.json")
        self.assertEqual(code, 1)
        self.assertIn("background workflow", err)

    def test_resume_after_partial_run(self):
        mock = self.server()
        self.run_cli("run", "--poll", "0.01", "--kind", "background")
        self.assertEqual(len(mock.prompts), 2)
        code, _, _ = self.run_cli("run", "--poll", "0.01")
        self.assertEqual(code, 0)
        self.assertEqual(len(mock.prompts), len(self.assets))

    def test_only_and_kind_filters(self):
        mock = self.server()
        self.run_cli("run", "--poll", "0.01", "--only", "spr_q_neutral", "--only", "bg_p*")
        self.assertEqual(sorted(os.listdir(self.out)), ["bg_pantry.png", "spr_q_neutral.png"])
        code, out, _ = self.run_cli("list", "--kind", "expression")
        self.assertEqual(len([l for l in out.splitlines() if l.startswith(("done", "pending"))]), 4)
        code, _, err = self.run_cli("run", "--only", "nothing_*")
        self.assertEqual(code, 1)
        self.assertIn("no assets match", err)

    def test_expression_without_its_neutral_is_refused(self):
        mock = self.server()
        code, _, err = self.run_cli("run", "--poll", "0.01", "--only", "spr_q_smile")
        self.assertEqual(code, 1)
        self.assertIn("enhanced neutral portrait first", err)
        self.assertEqual(mock.prompts, [])

    def test_server_down_is_reported(self):
        code, _, err = self.run_cli("--server", "http://127.0.0.1:9", "run")
        self.assertEqual(code, 1)
        self.assertIn("cannot reach ComfyUI", err)
        code, _, err = self.run_cli("--server", "http://127.0.0.1:9", "check")
        self.assertEqual(code, 2)

    def test_missing_model_is_rejected_by_the_server(self):
        self.server(checkpoints=["something_else.safetensors"])
        code, _, err = self.run_cli("run", "--poll", "0.01", "--only", "bg_axis")
        self.assertEqual(code, 1)
        self.assertIn("sd_xl_base_1.0.safetensors", err)
        self.assertFalse(os.path.exists(os.path.join(self.out, "bg_axis.png")))

    def test_execution_error_keeps_going_and_fails_at_the_end(self):
        mock = self.server()
        mock.fail_execution = True
        code, out, err = self.run_cli("run", "--poll", "0.01", "--kind", "background")
        self.assertEqual(code, 1)
        self.assertIn("CUDA out of memory", err)
        self.assertIn("2 failed", out)
        mock.prompts.clear()
        code, _, _ = self.run_cli("run", "--poll", "0.01", "--kind", "background", "--fail-fast")
        self.assertEqual(len(mock.prompts), 1)

    def test_timeout(self):
        mock = self.server()
        mock.never_finish = True
        code, _, err = self.run_cli("run", "--poll", "0.01", "--timeout", "0.2", "--only", "bg_axis")
        self.assertEqual(code, 1)
        self.assertIn("timed out", err)

    def test_dry_run_needs_no_server(self):
        folder = os.path.join(self.tmp.name, "dry")
        code, out, err = self.run_cli("--server", "http://127.0.0.1:9", "run", "--dry-run", folder)
        self.assertEqual(code, 0, err)
        self.assertEqual(len(os.listdir(folder)), len(self.assets))
        with open(os.path.join(folder, "bg_axis.json")) as f:
            prompt = json.load(f)
        self.assertEqual(enhance.validate_graph(prompt, self.specs), [])
        self.assertFalse(os.path.exists(self.out))

    def test_bad_config_value_is_caught_before_anything_is_sent(self):
        mock = self.server()
        cfg = dict(self.cfg, sampler="not_a_sampler")
        code, _, err = self.run_cli("run", "--poll", "0.01", "--only", "bg_axis", config=cfg)
        self.assertEqual(code, 1)
        self.assertIn("not_a_sampler", err)
        self.assertEqual(mock.prompts, [])
        cfg = {k: v for k, v in self.cfg.items() if k != "checkpoint"}
        code, _, err = self.run_cli("run", "--only", "bg_axis", config=cfg)
        self.assertIn("checkpoint", err)

    def test_missing_manifest(self):
        os.remove(os.path.join(self.guides, "manifest.json"))
        code, _, err = self.run_cli("run")
        self.assertEqual(code, 1)
        self.assertIn("export", err)

    def test_check(self):
        self.server()
        code, out, err = self.run_cli("check")
        self.assertEqual(code, 0, err)
        self.assertIn("reachable", out)

    def test_check_reports_missing_models_and_nodes(self):
        mock = self.server(controlnets=["other.safetensors"])
        mock.drop_nodes = {"Canny"}
        code, out, err = self.run_cli("check")
        self.assertEqual(code, 1)
        self.assertIn("diffusers_xl_canny_full.safetensors", out)
        self.assertIn("no node 'Canny'", out)


if __name__ == "__main__":
    unittest.main()
