# ComfyUI enhancement suite

The game paints every background and character itself, in code. This suite
runs those paintings through [ComfyUI](https://github.com/comfyanonymous/ComfyUI)
to repaint them with a diffusion model, **using the game's own art as the
guide**: each painting is both the starting image (img2img) and the structure
reference (ControlNet), so the result keeps the exact composition, layout and
characters the story was written around. The game then loads the results in
place of its own paintings, file by file.

Nothing here needs more than core ComfyUI nodes, a checkpoint and a
ControlNet. The runner is plain Python 3 (standard library only).

> **Not run on a GPU.** The workflows were written and checked against
> ComfyUI's source (commit `a2103ae`) and the runner is tested end to end
> against a mock server, but no image has actually been generated with them.
> Expect to tune the settings in `config.json` on first use.

The story editor (`cargo run --release -- --editor`, **Art** button) uses
the same workflows, settings and node checks from this folder, per story and
with a point-and-click queue. This runner does the built-in story in bulk
from the command line.

## Quick start

```bash
python3 comfyui/enhance.py export
```

Writes the guides to `comfyui/guides/` (needs the Rust toolchain; it runs the
game with `--export-guides`).

Start ComfyUI, then:

```bash
python3 comfyui/enhance.py check
```

```bash
python3 comfyui/enhance.py run
```

Results land in `assets/enhanced/`. Run the game from the repository root and
it picks them up.

## What you need

- A running ComfyUI, recent enough to have the `Canny`, `ImageBlur`,
  `JoinImageWithAlpha` and mask nodes built in. `check` tells you if a node is
  missing.
- A checkpoint in `ComfyUI/models/checkpoints/` and a matching ControlNet in
  `ComfyUI/models/controlnet/`. The defaults in `config.json` are
  `sd_xl_base_1.0.safetensors` and `diffusers_xl_canny_full.safetensors`
  (an SDXL canny ControlNet). These names are only defaults: set `checkpoint`
  and `controlnet` to whatever you have. A realism-tuned SDXL checkpoint will
  usually do better than the base model.
- For an SD 1.5 checkpoint, use an SD 1.5 canny ControlNet and change the
  sizes to about 912x512 for backgrounds and 512x696 for sprites (multiples
  of 8, same aspect ratios).

Check the licence of any checkpoint before shipping art made with it.

## What gets exported

| File | What it is |
|---|---|
| `bg_<name>.png` | Background painting, 1152x648 |
| `spr_<who>_<expr>.png` | Character sprite with transparency, 336x456 |
| `flat_<who>_<expr>.png` | The same sprite on neutral grey: the img2img start image |
| `mask_<who>_<expr>.png` | Silhouette, white where the character is |
| `face_<who>.png` | The region that changes between expressions |
| `manifest.json` | Every asset, its files and its text prompt |

7 backgrounds and 7 characters x 6 expressions: 49 assets.

## Other workflows

Besides the three described below, `workflows/` holds:

| File | Kind | What it does |
|---|---|---|
| `background_reimagine.json` | background | Starts from noise, not from the painting; ControlNet keeps only the outlines. |
| `sprite_reimagine.json` | sprite | The same for a neutral portrait, cut out as usual. |
| `background_refine.json` | background | A light pass (no ControlNet) over the current enhanced image, or the painting if there is none. |

```bash
python3 comfyui/enhance.py run --kind background --workflow background_reimagine.json --force
```

`--workflow` uses that file for everything selected; it must match the kind
of asset. A workflow file carries a `title`, a `kind`, and optionally
`defaults` (settings it prefers, which per-asset `overrides` still beat) and
`"guide": "enhanced"` (start from the current enhanced image).

## The three workflows

Templates are in `workflows/`, in ComfyUI's API format, with `$name`
placeholders that the runner fills from `config.json` and the manifest.

**`background.json`**

```
guide ─ scale ─┬─ VAE encode ──────────────────────────┐
               └─ Canny ─ ControlNet ─┐                │
prompts ──────────────────────────────┴─ KSampler (partial denoise) ─ decode ─ save
```

**`sprite_base.json`** (the neutral expression of each character). The same
chain starting from the sprite on grey, then the result is cut out:

```
decode ───────────────────────────────────────────┐
silhouette ─ scale ─ erode ─ blur ─ invert ─ JoinImageWithAlpha ─ save (RGBA)
```

The alpha channel is the *guide's* silhouette, pulled in a few pixels and
feathered. That is deliberate: it needs no extra nodes and guarantees the
sprite sits exactly where the game expects. The cost is that anything the
model paints outside the original outline is cut off, and if it shrinks the
figure a sliver of grey can show at the edge. High ControlNet strength keeps
the outline faithful; if you see grey fringes, make `alpha_grow` more
negative.

**`sprite_expression.json`** (the other five expressions). Generating each
expression separately would give six slightly different people. Instead:

```
enhanced neutral ─ scale ─ VAE encode ─ noise mask (face only) ─ KSampler ─ decode ─┐
expression guide ─ Canny ─ ControlNet ──────────────────────────────┘               │
enhanced neutral ─────────────────────────────── composite (face mask) ─────────────┘ ─ cut out ─ save
```

Only the face region is re-noised, steered by the edges of the expression
guide and the expression prompt, with the character's own seed. The new face
is then pasted back over the untouched neutral, so hair, clothes and identity
cannot drift. Neutral portraits are always generated first.

## Tuning (`config.json`)

Settings merge in this order: top level, then `kinds.<kind>`, then any
`overrides` entry whose key matches the asset id (`*` wildcards allowed).
Expressions inherit the `sprite` settings before applying `expression`.

| Setting | Effect |
|---|---|
| `denoise` | How far from the guide the model may go. Lower (0.4) stays close to the game's painting; higher (0.75) repaints more freely. |
| `control_strength`, `control_end` | How firmly edges are held, and for what fraction of the steps. Lower them if results look traced or stiff. |
| `canny_low`, `canny_high` | Edge sensitivity. The guides are softly shaded, so the thresholds are low. |
| `control_preprocess` | `canny`, or `none` to hand the guide to the ControlNet unprocessed (for tile or depth-style models). |
| `style_prefix`, `style_suffix`, `negative` | Wrapped round each asset's own prompt. This is where the target look is set. |
| `width`, `height` | Generation size. Keep multiples of 8 and the game's aspect ratio. |
| `alpha_grow`, `alpha_feather`, `alpha_sigma` | Sprite cut-out: erosion (negative) and edge softness. |
| `face_grow`, `face_feather`, `face_sigma` | Margin and seam softness of the repainted face. |
| `sampler`, `scheduler`, `steps`, `cfg`, `seed` | As in ComfyUI. Each character gets its own seed derived from `seed`. |

## Working asset by asset

```bash
python3 comfyui/enhance.py list
```

```bash
python3 comfyui/enhance.py run --only 'bg_*'
```

```bash
python3 comfyui/enhance.py run --only spr_q_neutral --force
```

- A run skips anything already in `assets/enhanced/`; `--force` redoes it.
  An interrupted run can simply be started again.
- If you redo a neutral portrait, redo that character's expressions too
  (`--only 'spr_q_*' --force`), since they are built from it.
- `--kind background|sprite|expression` filters by type.
- `sheet` writes before/after images to `comfyui/review/` (needs Pillow).
- Delete a file from `assets/enhanced/` to go back to the game's own painting
  for that asset. `cargo run --release -- --procedural` ignores them all.

## Using the workflows in the ComfyUI editor

```bash
python3 comfyui/enhance.py run --dry-run comfyui/resolved
```

writes one fully filled-in workflow per asset without contacting a server.
Copy the contents of `comfyui/guides/` into `ComfyUI/input/`, then open a
resolved file in the editor and queue it. The files are API-format JSON,
which recent ComfyUI front ends can open directly; I have not checked that
against a running editor. Expression
workflows also expect the enhanced neutral in `ComfyUI/input/` under the name
`enhanced_spr_<who>_neutral.png`.

## Tests

```bash
python3 -m unittest discover comfyui/tests
```

No network or GPU. A mock ComfyUI server validates every submitted graph
against `node_specs.json` (node names, input names, types, ranges, links and
model names) and the tests cover a full run, resuming, filters, ordering and
the failure paths. To re-check `node_specs.json` against a newer ComfyUI:

```bash
python3 comfyui/enhance.py verify-specs --comfy-src /path/to/ComfyUI
```

## Troubleshooting

| Message | Cause |
|---|---|
| `cannot reach ComfyUI` | It is not running, or is on another address: pass `--server URL` or edit `server`. |
| `'x.safetensors' is not available` | The file named in `config.json` is not in the server's models folder. |
| `this ComfyUI has no node 'Canny'` | ComfyUI is too old; update it. |
| `needs the enhanced neutral portrait first` | You asked for an expression before its neutral exists. |
| `the result has no alpha channel` | The sprite workflow was edited so it no longer ends in `JoinImageWithAlpha`. |
| `timed out` | Generation is slower than `--timeout` (default 1800 s per image). |
