#!/usr/bin/env bash
# Build the Tuskar Docker image and hot-swap the running container.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

# ── colours ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'
BOLD='\033[1m'; RESET='\033[0m'

info()    { echo -e "  ${CYAN}•${RESET} $*"; }
success() { echo -e "  ${GREEN}✓${RESET} $*"; }
warn()    { echo -e "  ${YELLOW}!${RESET} $*"; }
die()     { echo -e "  ${RED}✗${RESET} $*" >&2; exit 1; }

# ── flags ─────────────────────────────────────────────────────────────────────
NO_CACHE=""
NO_RESTART=""
for arg in "$@"; do
  case "$arg" in
    --no-cache)   NO_CACHE="--no-cache" ;;
    --no-restart) NO_RESTART=1 ;;
    -h|--help)
      echo "Usage: $0 [--no-cache] [--no-restart]"
      echo "  --no-cache    Pass --no-cache to docker compose build"
      echo "  --no-restart  Build only; do not restart the running container"
      exit 0 ;;
    *) die "Unknown argument: $arg" ;;
  esac
done

echo ""
echo -e "${BOLD}Tuskar — build${RESET}"
echo "──────────────────────────────────────"

# ── prefer 'docker compose' (v2) ─────────────────────────────────────────────
if docker compose version >/dev/null 2>&1; then
  COMPOSE="docker compose"
elif command -v docker-compose >/dev/null 2>&1; then
  COMPOSE="docker-compose"
else
  die "Docker Compose not found."
fi

# ── build ─────────────────────────────────────────────────────────────────────
info "Building Tuskar image${NO_CACHE:+ (--no-cache)}…"
# shellcheck disable=SC2086
$COMPOSE build $NO_CACHE tus
success "Image built"

# ── restart ───────────────────────────────────────────────────────────────────
if [[ -z "$NO_RESTART" ]]; then
  # Only restart if the container is already running — avoids a cold-start side-effect.
  if $COMPOSE ps --status running tus 2>/dev/null | grep -q "tus"; then
    info "Restarting tus container with new image…"
    $COMPOSE up -d --no-deps tus
    success "Container restarted"
  else
    warn "tus container is not running — skipping restart (run ./start.sh to start)"
  fi
else
  warn "Skipping container restart (--no-restart)"
fi

# ── done ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}Build complete.${RESET}"
echo ""
echo "  Start:   ${BOLD}./start.sh${RESET}"
echo "  Rebuild: ${BOLD}./build.sh${RESET}  ${CYAN}(rebuilds + restarts automatically)${RESET}"
echo "  Full:    ${BOLD}./build.sh --no-cache${RESET}  ${CYAN}(force clean rebuild)${RESET}"
echo ""
