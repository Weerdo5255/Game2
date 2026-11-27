#!/bin/bash
# Check of the VR build without a headset: runs the game with --vr against
# the Monado OpenXR runtime's simulated headset, under a virtual display,
# and saves what the headset's two eyes were shown.
#
#   cargo build --features vr && tools/vr_check.sh [output.png]
#
# Needs: monado-service, libopenxr-loader1, libopenxr1-monado, xvfb,
# mesa-vulkan-drivers, x11-apps (xwd) and imagemagick (convert).
# It proves the OpenXR session, the eye cameras and the text panel; it says
# nothing about controllers or comfort, which need a real headset.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/docs/world_vr.png}"
case "$out" in /*) ;; *) out="$PWD/$out" ;; esac
tmp="$(mktemp -d)"
trap 'pkill -f monado-service 2>/dev/null; rm -rf "$tmp"' EXIT
{ echo "render 3d"; cat "$root/assets/story.txt"; } > "$tmp/story.txt"
# the engine looks for the loader under its unversioned name
mkdir -p "$tmp/lib" "$tmp/rt" && chmod 700 "$tmp/rt"
loader="$(ls /usr/lib/*/libopenxr_loader.so.1 2>/dev/null | head -1)"
[ -n "$loader" ] || { echo "FAIL: no OpenXR loader installed (libopenxr-loader1)"; exit 1; }
ln -sf "$loader" "$tmp/lib/libopenxr_loader.so"
cat > "$tmp/inner.sh" <<INNER
#!/bin/bash
export XDG_RUNTIME_DIR="$tmp/rt" LD_LIBRARY_PATH="$tmp/lib" XRT_COMPOSITOR_FORCE_XCB=1 XRT_COMPOSITOR_XCB_FULLSCREEN=0
(sleep 300 | monado-service > "$tmp/monado.log" 2>&1) &
sleep 5
(sleep 17; xwd -root -silent > "$tmp/screen.xwd" 2>/dev/null) &
cd "$tmp" && timeout 110 "$root/target/debug/below-stairs" --story "$tmp/story.txt" --start ch5 --vr --world-shot "$tmp/window.png" --world-shot-lines 4 --silent > "$tmp/game.log" 2>&1
INNER
chmod +x "$tmp/inner.sh"
xvfb-run -a -s "-screen 0 2560x1600x24" "$tmp/inner.sh" || true
fail=0
check() { if grep -q "$2" "$tmp/game.log"; then echo "ok: $1"; else echo "FAIL: $1"; fail=1; fi; }
check "an OpenXR runtime was found" "Loaded OpenXR runtime"
check "a headset was found" "Using system"
check "both eyes have a camera" "XrCamera resolution"
check "the controller actions were attached" "attaching 1 sessions"
if grep -qi "panicked" "$tmp/game.log"; then echo "FAIL: the game crashed"; grep -i -A3 panicked "$tmp/game.log" | head -8; fail=1; else echo "ok: no crash"; fi
if [ -s "$tmp/window.png" ]; then echo "ok: the window still shows the game"; else echo "FAIL: no picture of the window"; fail=1; fi
if [ -s "$tmp/screen.xwd" ] && convert "$tmp/screen.xwd" -crop 640x360+0+0 +repage -resize 1152x648 "$out" 2>/dev/null; then echo "ok: the headset's view is in $out"; else echo "FAIL: could not capture the headset's view"; fail=1; fi
exit $fail
