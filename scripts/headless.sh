#!/usr/bin/env bash
# Run a command against a private headless sway, so GUI windows never reach the desktop.
# Usage: scripts/headless.sh <cmd> [args...]. PC_HEADLESS=0 runs the command as-is (visible window).
set -euo pipefail

if [[ "${PC_HEADLESS:-1}" == 0 ]]; then
    exec "$@"
fi

command -v sway >/dev/null || { echo "headless: sway not found (or set PC_HEADLESS=0)" >&2; exit 1; }

# Private runtime dir: sway's Wayland and IPC sockets land here, not next to the user's session.
run=$(mktemp -d /tmp/photo-cruiser-wl.XXXXXX)
conf="$run/sway.conf"
echo "output HEADLESS-1 resolution 1600x1000" >"$conf"

env -u DISPLAY -u WAYLAND_DISPLAY -u SWAYSOCK -u I3SOCK XDG_RUNTIME_DIR="$run" \
    WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
    sway -c "$conf" >"$run/sway.log" 2>&1 &
sway_pid=$!
trap 'kill "$sway_pid" 2>/dev/null; wait "$sway_pid" 2>/dev/null; trash-put "$run" 2>/dev/null || true' EXIT

for _ in $(seq 50); do
    sock=$(find "$run" -maxdepth 1 -type s -name 'wayland-*' -print -quit)
    [[ -n "$sock" ]] && break
    kill -0 "$sway_pid" 2>/dev/null || { cat "$run/sway.log" >&2; exit 1; }
    sleep 0.1
done
[[ -n "$sock" ]] || { echo "headless: sway socket did not appear" >&2; cat "$run/sway.log" >&2; exit 1; }

# An absolute WAYLAND_DISPLAY points the app at the socket; its own XDG_RUNTIME_DIR stays untouched.
env -u DISPLAY WAYLAND_DISPLAY="$sock" "$@"
