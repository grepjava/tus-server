#!/usr/bin/env bash
# Start all Tuskar services (Tuskar + Prometheus + Grafana).
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

# ── colours ───────────────────────────────────────────────────────────────────
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'
BOLD='\033[1m'; RESET='\033[0m'

# ── compose binary ────────────────────────────────────────────────────────────
if docker compose version >/dev/null 2>&1; then
  COMPOSE="docker compose"
elif command -v docker-compose >/dev/null 2>&1; then
  COMPOSE="docker-compose"
else
  echo "Docker Compose not found." >&2; exit 1
fi

# ── load .env ─────────────────────────────────────────────────────────────────
if [[ -f "$DIR/.env" ]]; then
  set -a
  # shellcheck disable=SC1090
  source <(grep -v '^\s*#' "$DIR/.env" | grep -v '^\s*$')
  set +a
fi

BASE_URL="${BASE_URL:-http://localhost:3000}"
GRAFANA_URL="${GRAFANA_URL:-http://localhost:3001}"
BIND_PORT="${BIND_PORT:-3000}"

# ── note if image needs building ─────────────────────────────────────────────
if ! $COMPOSE images tus 2>/dev/null | grep -q tus; then
  echo -e "${YELLOW}Tuskar image not found — building now (first run takes a few minutes)…${RESET}"
fi

# ── start ─────────────────────────────────────────────────────────────────────
echo "Starting Tuskar…"
$COMPOSE up -d

# ── wait for Tuskar health ────────────────────────────────────────────────────
echo -n "Waiting for Tuskar to be ready"
for i in $(seq 1 30); do
  if curl -fs "http://localhost:${BIND_PORT}/api/health" >/dev/null 2>&1; then
    echo ""
    break
  fi
  echo -n "."
  sleep 1
  if [[ $i -eq 30 ]]; then
    echo ""
    echo -e "${YELLOW}Tuskar did not respond in 30s — check logs: $COMPOSE logs tus${RESET}"
  fi
done

# ── URLs ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}Tuskar is running${RESET}"
echo ""
echo -e "  ${GREEN}Console${RESET}     ${BASE_URL}"
echo -e "  ${GREEN}Dashboard${RESET}   ${GRAFANA_URL}  (Grafana — may take ~10s on first boot)"
echo -e "  ${CYAN}Prometheus${RESET}  http://localhost:9090"
echo ""
echo -e "  Logs:   ${BOLD}$COMPOSE logs -f${RESET}"
echo -e "  Stop:   ${BOLD}./stop.sh${RESET}"
echo ""
