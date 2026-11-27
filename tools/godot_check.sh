#!/bin/bash
# Checks the Godot project against the Rust game: exports each story, plays it
# in Godot with no window along several lines of choices, and compares how it
# went (the ending, who came and went, what changed hands) with the Rust
# game playing the same choices.
#
#   cargo build && tools/godot_check.sh [path-to-godot-4]
#
# Needs Godot 4.3 or later (the editor binary: it imports the project first).
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
godot="${1:-${GODOT:-godot}}"
exe="$root/target/debug/below-stairs"
command -v "$godot" >/dev/null 2>&1 || { echo "FAIL: no Godot at '$godot' (pass its path, or set GODOT)"; exit 1; }
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail=0
# story file, then the lines of choices to try
check() {
  story="$1"; shift
  project="$tmp/$(basename "$story" .txt)"
  (cd "$root" && "$exe" --story "$story" --export-godot "$project" > "$tmp/export.log" 2>&1) || { echo "FAIL: export of $story"; cat "$tmp/export.log"; fail=1; return; }
  "$godot" --headless --path "$project" --import > "$tmp/import.log" 2>&1 || true
  if grep -qiE "SCRIPT ERROR|Parse Error|Failed to load script" "$tmp/import.log"; then echo "FAIL: Godot reports script errors importing $story"; grep -iE -A3 "SCRIPT ERROR|Parse Error|Failed to load" "$tmp/import.log" | head -20; fail=1; return; fi
  for picks in "$@"; do
    want="$(cd "$root" && "$exe" --story "$story" --autoplay "$picks" --silent 2>/dev/null | grep -E '^(schedule|exchanges|ending):' | sed -E 's/^(ending: .*) \(peace .*$/\1/')"
    got="$("$godot" --headless --path "$project" --script res://tests/run_story.gd -- "$picks" 2> "$tmp/run.log" | grep -E '^(schedule|exchanges|ending):')" || true
    if [ "$want" = "$got" ] && [ -n "$got" ]; then echo "ok: $(basename "$story") [$picks]: $(echo "$got" | tail -1)";
    else echo "FAIL: $(basename "$story") [$picks]"; echo "  the Rust game: $want"; echo "  Godot:         $got"; grep -iE "ERROR" "$tmp/run.log" | head -5; fail=1; fi
  done
}
check assets/story.txt "0,0,1,0,0,2,3" "0" "1,1,1,1,1,1,1,1" "2,2,0,1,2,0,1,3" "0,1,2,0,1,2,0,1,2"
check assets/example_schedule.txt "0" "1"
check assets/example_holdings.txt "0" "1" "2"
exit $fail
