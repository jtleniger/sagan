#!/usr/bin/env sh
#
# Run the development stack the way a deployment runs it: the web server in one
# process, the scheduler in another. Periodic work then runs in the scheduler's
# own child process, not in the one serving requests, and killing either process
# leaves the other alone.
#
# `cargo loco start --all` runs both in one process instead; this script exists to
# show the difference. Ctrl-C stops both.
set -eu

cd "$(dirname "$0")/.."

pids=""
trap 'kill $pids 2>/dev/null || true' EXIT INT TERM

cargo loco start &
pids="$pids $!"

# `--worker --scheduler` is one process without an HTTP server: a second server
# would fight for the port, and the worker flag is what gives a non-server start
# mode a process to live in.
cargo loco start --worker --scheduler &
pids="$pids $!"

wait
