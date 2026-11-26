#!/bin/sh
# Dev loop helper: kills any stale rosterbook process before building and
# running a fresh one.
#
# Why this is needed: rebuilding while the app is running unlinks the old
# binary on disk, but the running process keeps it mapped (shows as
# "(deleted)" in /proc/<pid>/exe) and keeps serving the old code. Tauri's
# single-instance plugin then forwards any new launch to that stale process
# instead of starting a new one, so the rebuild silently has no effect until
# the old process is killed by hand.
#
#   sh dev/run.sh
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
bin="$here/target/debug/rosterbook"

for pid in $(pgrep -f "target/(debug|release)/rosterbook" 2>/dev/null || true); do
    exe=$(readlink -f "/proc/$pid/exe" 2>/dev/null || true)
    case "$exe" in
        *"(deleted)") echo "Killing stale rosterbook process $pid (running a deleted binary)"; kill "$pid" ;;
    esac
done

cd "$here"
cargo build -p rosterbook
exec "$bin" "$@"
