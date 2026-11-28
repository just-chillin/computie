#!/usr/bin/env bash
# Runs computie on a headless Wayland compositor and saves a screenshot.
# usage: scripts/shot.sh <out.png> [style] [mode] [folder-to-scan] [seconds]
# Needs: sway, grim, a software Vulkan driver (mesa-vulkan-drivers).
set -u
out=$1; style=${2:-linux}; mode=${3:-light}; folder=${4:-}; wait=${5:-8}
run=${XDG_RUNTIME_DIR:-/tmp/computie-shot-run}
mkdir -p "$run"; chmod 700 "$run"
export XDG_RUNTIME_DIR=$run
if ! pgrep -x sway >/dev/null; then
  conf=$run/sway.conf
  printf 'output HEADLESS-1 resolution 1100x720\ndefault_border none\n' > "$conf"
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 setsid sway -c "$conf" >"$run/sway.log" 2>&1 < /dev/null &
  sleep 3
fi
sock=$(ls "$run" | grep -E '^wayland-[0-9]+$' | head -1)
bin=$(dirname "$0")/../target/debug/computie
if [ -n "$folder" ]; then
  env -u DISPLAY WAYLAND_DISPLAY=$sock COMPUTIE_STYLE=$style COMPUTIE_MODE=$mode timeout $((wait+6)) "$bin" "$folder" >"$run/app.log" 2>&1 &
  app=$!
else
  env -u DISPLAY WAYLAND_DISPLAY=$sock COMPUTIE_STYLE=$style COMPUTIE_MODE=$mode timeout $((wait+6)) "$bin" >"$run/app.log" 2>&1 &
  app=$!
fi
sleep "$wait"
WAYLAND_DISPLAY=$sock grim "$out"
kill "$app" 2>/dev/null
