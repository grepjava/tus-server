#!/usr/bin/env bash
# Stop all Tuskar services.
#   ./stop.sh           — stop containers, keep volumes (data preserved)
#   ./stop.sh --clean   — stop containers and remove all volumes (full reset)
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

YELLOW='\033[1;33m'; BOLD='\033[1m'; RESET='\033[0m'

if docker compose version >/dev/null 2>&1; then
  COMPOSE="docker compose"
elif command -v docker-compose >/dev/null 2>&1; then
  COMPOSE="docker-compose"
else
  echo "Docker Compose not found." >&2; exit 1
fi

CLEAN=false
for arg in "$@"; do
  [[ "$arg" == "--clean" ]] && CLEAN=true
done

if [[ "$CLEAN" == true ]]; then
  echo -e "${YELLOW}--clean: all volumes (uploads, database, Prometheus data, Grafana data) will be deleted.${RESET}"
  read -r -p "  Are you sure? [y/N] " confirm
  [[ "$confirm" =~ ^[Yy]$ ]] || { echo "Aborted."; exit 0; }
  echo "Stopping Tuskar and removing volumes…"
  $COMPOSE down --volumes
  echo "All data removed."
else
  echo "Stopping Tuskar…"
  $COMPOSE down
  echo -e "Stopped. Data is preserved. Run ${BOLD}./start.sh${RESET} to restart."
fi
