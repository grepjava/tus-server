#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$DIR/.server.pid"
LOG_FILE="$DIR/server.log"

if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
  echo "Already running (PID $(cat "$PID_FILE"))."
  exit 0
fi

# Pick binary: prefer release over debug
BINARY=""
for candidate in "$DIR/target/release/tus-server" "$DIR/target/debug/tus-server"; do
  if [[ -x "$candidate" ]]; then
    BINARY="$candidate"
    break
  fi
done

if [[ -z "$BINARY" ]]; then
  echo "No binary found. Run one of:"
  echo "  cargo build --release   (production)"
  echo "  cargo build             (development)"
  exit 1
fi

# Load .env if present (skip comments and blanks)
if [[ -f "$DIR/.env" ]]; then
  set -a
  # shellcheck disable=SC1090
  source <(grep -v '^\s*#' "$DIR/.env" | grep -v '^\s*$')
  set +a
fi

export DATABASE_URL="${DATABASE_URL:-tus.db}"
export STORAGE_DIR="${STORAGE_DIR:-uploads}"
export BASE_URL="${BASE_URL:-http://localhost:3000}"
export BIND_ADDR="${BIND_ADDR:-0.0.0.0:3000}"
export RUST_LOG="${RUST_LOG:-info}"

cd "$DIR"
mkdir -p "$STORAGE_DIR"

nohup "$BINARY" >> "$LOG_FILE" 2>&1 &
echo $! > "$PID_FILE"

echo "TUS server started  (PID $!, binary: $(basename "$BINARY"))"
echo "Dashboard:          $BASE_URL"
echo "Logs:               $LOG_FILE"
