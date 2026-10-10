#!/usr/bin/env bash
# Live development: rebuild on every save under crates/, then restart the app on the
# photo it was showing. A failed build leaves the running window alone. Closing the
# window or Ctrl+C ends the session.
# Usage: scripts/dev.sh [dir]   (or `just dev [dir]`)
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
dir=$(realpath "${1:-$root/fixtures}")
bin="$root/target/debug/photo-cruiser"
resume=$(mktemp /tmp/photo-cruiser-resume.XXXXXX)
stamp=$(mktemp /tmp/photo-cruiser-stamp.XXXXXX)
pid=

stop() {
    [[ -n $pid ]] || return 0
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    pid=
}
cleanup() {
    stop
    trash-put "$resume" "$stamp" 2>/dev/null || rm -f "$resume" "$stamp"
}
trap cleanup EXIT
trap 'exit 0' INT TERM

build() {
    local t=$SECONDS
    if (cd "$root" && cargo build -q -p photo-cruiser-app); then
        echo "dev: built in $((SECONDS - t)) s"
    else
        echo "dev: build failed; keeping the running app" >&2
        return 1
    fi
}

start() {
    stop
    local target
    target=$(cat "$resume")
    [[ -n $target && -e $target ]] || target=$dir
    PC_RESUME_FILE="$resume" "$bin" "$target" &
    pid=$!
}

build || exit 1
start
echo "dev: watching crates/ (Ctrl+C or close the window to stop)"
touch "$stamp"
while true; do
    sleep 0.5
    if [[ -n $pid ]] && ! kill -0 "$pid" 2>/dev/null; then
        echo "dev: app exited; stopping"
        pid=
        exit 0
    fi
    changed=$(find "$root/crates" \( -name '*.rs' -o -name '*.toml' \) -newer "$stamp" -print -quit)
    [[ -n $changed ]] || continue
    sleep 0.2 # let editors finish writing
    touch "$stamp"
    build && start
done
