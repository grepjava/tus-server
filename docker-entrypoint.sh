#!/usr/bin/env bash
set -e

# If the AV processor is enabled, ensure ClamAV signatures are present and fresh.
# This runs as root because freshclam needs to write to the signature volume.
if [[ "${PROCESSORS:-}" == *"av"* ]]; then
  SIGDIR="${AV_CLAMAV_SIGDIR:-/var/lib/clamav}"
  mkdir -p "$SIGDIR"

  if [[ ! -f "$SIGDIR/main.cvd" && ! -f "$SIGDIR/main.cld" ]]; then
    echo "[tuskar] No ClamAV signatures found — downloading now (this may take a few minutes)…"
  else
    echo "[tuskar] Refreshing ClamAV signatures…"
  fi

  freshclam \
    --quiet \
    --no-warnings \
    --datadir="$SIGDIR" \
    || echo "[tuskar] Warning: freshclam failed — AV scans will use existing signatures or fail if none exist."
fi

# Ensure the data and upload directories are writable by the app user,
# then drop root privileges before starting the server.
mkdir -p /data /uploads
chown -R 1000:0 /data /uploads
exec gosu 1000 ./tus-server
