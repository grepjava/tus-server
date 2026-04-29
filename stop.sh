#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$DIR/.server.pid"

if [[ ! -f "$PID_FILE" ]]; then
  echo "No PID file found — server may not be running."
  exit 0
fi

PID=$(cat "$PID_FILE")

if kill -0 "$PID" 2>/dev/null; then
  kill "$PID"
  # Wait up to 5s for graceful shutdown
  for i in $(seq 1 10); do
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.5
  done
  if kill -0 "$PID" 2>/dev/null; then
    echo "Process did not exit — sending SIGKILL."
    kill -9 "$PID"
  fi
  rm -f "$PID_FILE"
  echo "TUS server stopped (PID $PID)."
else
  echo "Process $PID not running — cleaning up stale PID file."
  rm -f "$PID_FILE"
fi
