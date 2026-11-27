#!/bin/bash
# Smoke test of the 3D world, and the pictures for the README: plays the
# built-in story with every place in 3D (one, the study, left as a painting)
# and takes a picture in each place.
#
#   cargo build --features world3d && tools/world3d_shots.sh [output-dir]
#
# Needs a display and a GPU, or stand-ins for both: under xvfb-run with
# Mesa's software Vulkan (lavapipe) it runs anywhere, slowly.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/docs}"
exe="$root/target/debug/below-stairs"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$out"
out="$(cd "$out" && pwd)"
{ echo "render 3d"; echo "render study 2d"; cat "$root/assets/story.txt"; } > "$tmp/story.txt"
run() { if [ -n "$DISPLAY" ] || [ -n "$WAYLAND_DISPLAY" ]; then "$@"; else xvfb-run -a -s "-screen 0 1280x800x24" "$@"; fi; }
fail=0
# scene to start from, name of the picture, lines to show first
for shot in "start pantry 2" "ch1 axis 4" "ch2 dock 6" "ch4 vista 4" "ch5 ballroom 4" "summit study 3"; do
  set -- $shot
  # (taken beside, and moved in only once it exists: a failed run leaves the old picture alone)
  rm -f "$tmp/shot.png"
  (cd "$tmp" && run "$exe" --story "$tmp/story.txt" --start "$1" --world-shot "$tmp/shot.png" --world-shot-lines "$3" --silent > "$tmp/$2.log" 2>&1) || true
  if [ -s "$tmp/shot.png" ]; then mv "$tmp/shot.png" "$out/world_$2.png"; echo "ok: $2"; else echo "FAIL: $2"; tail -5 "$tmp/$2.log"; fail=1; fi
done
exit $fail
