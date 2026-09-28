#!/usr/bin/env sh
#
# Run the development stack the way a deployment runs it: the web server in one
# process, the worker and the scheduler in another. Jobs then run in a process
# that is not the one serving requests, and killing either process leaves the
# other — and the queued jobs — alone.
#
# `cargo loco start --all` runs all three in one process instead; this script
# exists to show the difference. Ctrl-C stops both.
set -eu

cd "$(dirname "$0")/.."

pids=""
trap 'kill $pids 2>/dev/null || true' EXIT INT TERM

cargo loco start &
pids="$pids $!"

# `--worker --scheduler` is one process without an HTTP server: Loco has no
# scheduler-only mode, and a second server would fight for the port.
cargo loco start --worker --scheduler &
pids="$pids $!"

wait
