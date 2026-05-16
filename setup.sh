#!/usr/bin/env bash
# First-time setup: check prerequisites, create .env, pull images, build Tuskar.
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

echo ""
echo -e "${BOLD}Tuskar — setup${RESET}"
echo "──────────────────────────────────────"

# ── 1. prerequisites ──────────────────────────────────────────────────────────
info "Checking prerequisites…"

command -v docker >/dev/null 2>&1 \
  || die "Docker not found. Install Docker Desktop or Docker Engine first."

# Prefer 'docker compose' (v2) over 'docker-compose' (v1)
if docker compose version >/dev/null 2>&1; then
  COMPOSE="docker compose"
elif command -v docker-compose >/dev/null 2>&1; then
  COMPOSE="docker-compose"
else
  die "Docker Compose not found. Install Docker Compose v2."
fi

success "Docker $(docker --version | cut -d' ' -f3 | tr -d ',')"
success "Compose  $($COMPOSE version --short 2>/dev/null || $COMPOSE version | head -1)"

# ── 2. .env file ──────────────────────────────────────────────────────────────
if [[ ! -f "$DIR/.env" ]]; then
  info "Creating .env from template…"
  cat > "$DIR/.env" <<'EOF'
# Tuskar environment — edit as needed before running ./start.sh

# Ports
BIND_PORT=3000
GRAFANA_PORT=3001

# Public URL of the Tuskar server (used in TUS Location headers)
BASE_URL=http://localhost:3000

# Public URL of Grafana (embedded in the Dashboard tab)
GRAFANA_URL=http://localhost:3001

# Required: password for the initial admin account (used on first startup only).
# Change this before going to production. After the first run you can update
# the password via the dashboard — this variable is no longer read.
ADMIN_PASSWORD=changeme

# Set to true when serving over HTTPS to add the Secure flag to session cookies.
# COOKIE_SECURE=true

# Optional: protect the TUS API with a bearer token
# API_KEY=changeme

# Optional: S3 storage backend
# STORAGE_BACKEND=s3
# S3_BUCKET=my-tuskar-bucket
# S3_FORCE_PATH_STYLE=false

# Optional: enable processors (comma-separated)
# Enabling "av" turns on ClamAV scanning. Signatures (~270 MB) are
# downloaded on first start and kept in the clamav-data Docker volume.
# PROCESSORS=av

# Log level: error | warn | info | debug | trace
RUST_LOG=info
EOF
  success ".env created"
else
  success ".env already exists — skipping"
fi

# ── 3. pull third-party images ────────────────────────────────────────────────
info "Pulling Prometheus and Grafana images…"
$COMPOSE pull prometheus grafana
success "Images pulled"

# ── 4. build Tuskar ───────────────────────────────────────────────────────────
info "Building Tuskar image (this takes a minute on first run)…"
$COMPOSE build tus
success "Tuskar image built"

# ── done ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}Setup complete.${RESET}"
echo ""
echo "  Start:   ${BOLD}./start.sh${RESET}"
echo "  Stop:    ${BOLD}./stop.sh${RESET}"
echo ""
