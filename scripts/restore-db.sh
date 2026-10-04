#!/usr/bin/env bash
#
# Restore the production CourseMaster database from a snapshot taken by
# ./scripts/backup-db.sh. DESTRUCTIVE: the current database is replaced.
#
#   ./scripts/restore-db.sh coursemaster-20261002-201433.db
#
# How it works, and why: the container is stopped for the swap rather than
# written underneath, because replacing the file beneath a live SQLite
# connection corrupts it. The swap itself is done on the VPS *host*, against
# the volume's real path (resolved from `docker inspect`), because the stale
# `-wal`/`-shm` sidecars must be deleted while nothing is running — they
# describe the database being replaced, and leaving them would have SQLite
# replay a journal belonging to a file that is gone. A stopped container
# cannot be `docker exec`'d, so host-side file operations are the only way to
# do that part correctly.
#
# Before the swap, the CURRENT database is itself snapshotted, so running this
# against the wrong file is also recoverable.
#
# Environment overrides match backup-db.sh:
#   CM_SSH_TARGET  CM_CONTAINER  CM_DB_PATH  CM_BACKUP_DIR
#   CM_ASSUME_YES=1  skips the confirmation prompt (for tests)
#   CM_SKIP_SITE_CHECK=1  skips the final public-URL check

set -euo pipefail

SSH_TARGET="${CM_SSH_TARGET:-root@95.216.166.108}"
CONTAINER="${CM_CONTAINER:-coursemaster-api-1}"
DB_PATH="${CM_DB_PATH:-/app/data/coursemaster.db}"
BACKUP_DIR="${CM_BACKUP_DIR:-/opt/coursemaster/backups}"

SNAPSHOT="${1:-}"
if [ -z "$SNAPSHOT" ]; then
  echo "usage: $0 <snapshot-filename>" >&2
  echo >&2
  echo "available on ${SSH_TARGET}:" >&2
  ssh "$SSH_TARGET" "ls -1t '${BACKUP_DIR}' 2>/dev/null || echo '  (none)'" >&2
  exit 2
fi

echo "This REPLACES the live database at ${CONTAINER}:${DB_PATH}"
echo "with ${BACKUP_DIR}/${SNAPSHOT}."
if [ "${CM_ASSUME_YES:-}" != "1" ]; then
  printf 'Type the snapshot filename to confirm: '
  read -r CONFIRM
  if [ "$CONFIRM" != "$SNAPSHOT" ]; then
    echo "Aborted — that did not match." >&2
    exit 1
  fi
fi

ssh "$SSH_TARGET" bash -s -- "$CONTAINER" "$DB_PATH" "$BACKUP_DIR" "$SNAPSHOT" <<'REMOTE'
set -euo pipefail
CONTAINER="$1"; DB_PATH="$2"; BACKUP_DIR="$3"; SNAPSHOT="$4"
SRC="${BACKUP_DIR}/${SNAPSHOT}"
DATA_DIR="$(dirname "$DB_PATH")"
DB_FILE="$(basename "$DB_PATH")"

[ -f "$SRC" ] || { echo "FATAL: no such snapshot: ${SRC}" >&2; exit 1; }

ensure_sqlite3() {
  docker exec "$CONTAINER" sh -c 'command -v sqlite3 >/dev/null 2>&1 || {
    apt-get update -qq >/dev/null 2>&1
    apt-get install -y -qq sqlite3 >/dev/null 2>&1
  }'
}

if [ -z "$(docker ps -q -f "name=^${CONTAINER}$")" ]; then
  echo "FATAL: container ${CONTAINER} is not running." >&2
  exit 1
fi
ensure_sqlite3

# Verify the snapshot BEFORE touching anything. The VPS host has no sqlite3,
# so this runs inside the container against a staged copy — restoring an
# unreadable snapshot would trade a working database for a broken one.
echo "==> Verifying the snapshot"
docker cp "$SRC" "${CONTAINER}:/tmp/restore-candidate.db"
CHECK="$(docker exec "$CONTAINER" sqlite3 /tmp/restore-candidate.db 'PRAGMA integrity_check;')"
if [ "$CHECK" != "ok" ]; then
  echo "FATAL: snapshot is corrupt: ${CHECK}" >&2
  docker exec "$CONTAINER" rm -f /tmp/restore-candidate.db || true
  exit 1
fi
SUMMARY="$(docker exec "$CONTAINER" sqlite3 /tmp/restore-candidate.db "
  SELECT 'courses='        || (SELECT COUNT(*) FROM courses)
      || ' assignments='   || (SELECT COUNT(*) FROM assignments)
      || ' schema_version='|| (SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations);")"
docker exec "$CONTAINER" rm -f /tmp/restore-candidate.db
echo "    integrity_check: ok"
echo "    snapshot contains: ${SUMMARY}"

SAFETY="${BACKUP_DIR}/pre-restore-$(date -u +%Y%m%d-%H%M%S).db"
echo "==> Snapshotting the CURRENT database first -> ${SAFETY}"
mkdir -p "$BACKUP_DIR"
docker exec "$CONTAINER" sqlite3 "$DB_PATH" ".backup '/tmp/pre-restore.db'"
docker cp "${CONTAINER}:/tmp/pre-restore.db" "$SAFETY"
docker exec "$CONTAINER" rm -f /tmp/pre-restore.db

# Resolve where the data directory actually lives on this host. `.Source` is
# the host path for a bind mount AND for a named volume (the latter under
# /var/lib/docker/volumes/<name>/_data), so one lookup covers both.
HOST_DATA_DIR="$(docker inspect "$CONTAINER" \
  --format '{{range .Mounts}}{{if eq .Destination "'"$DATA_DIR"'"}}{{.Source}}{{end}}{{end}}')"
if [ -z "$HOST_DATA_DIR" ] || [ ! -d "$HOST_DATA_DIR" ]; then
  echo "FATAL: could not resolve the host path for ${DATA_DIR} in ${CONTAINER}." >&2
  echo "       Mounts are:" >&2
  docker inspect "$CONTAINER" --format '{{range .Mounts}}       {{.Destination}} <- {{.Source}}{{"\n"}}{{end}}' >&2
  exit 1
fi
echo "    data dir on host: ${HOST_DATA_DIR}"

echo "==> Stopping ${CONTAINER}"
docker stop "$CONTAINER" >/dev/null

echo "==> Swapping in the snapshot"
cp "$SRC" "${HOST_DATA_DIR}/${DB_FILE}"
# These belong to the database just replaced. With WAL enabled, leaving them
# behind means SQLite replays a journal for a file that no longer exists.
rm -f "${HOST_DATA_DIR}/${DB_FILE}-wal" "${HOST_DATA_DIR}/${DB_FILE}-shm"
echo "    swapped, and removed any stale -wal/-shm sidecars"

echo "==> Starting ${CONTAINER}"
docker start "$CONTAINER" >/dev/null
sleep 3

if [ -z "$(docker ps -q -f "name=^${CONTAINER}$")" ]; then
  echo "FATAL: ${CONTAINER} did not stay up. Last log lines:" >&2
  docker logs --tail 30 "$CONTAINER" >&2
  echo "       The database you replaced is at ${SAFETY}" >&2
  exit 1
fi

echo "==> Post-restore state"
ensure_sqlite3
docker exec "$CONTAINER" sqlite3 "$DB_PATH" "
  SELECT '    ' || 'courses='  || (SELECT COUNT(*) FROM courses)
      || ' assignments='       || (SELECT COUNT(*) FROM assignments)
      || ' extractions='       || (SELECT COUNT(*) FROM syllabus_extractions)
      || ' schema_version='    || (SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations);"
docker ps --filter "name=^${CONTAINER}$" --format '    {{.Names}} {{.Status}}'
echo "    safety copy of the replaced database: ${SAFETY}"
REMOTE

if [ "${CM_SKIP_SITE_CHECK:-}" != "1" ]; then
  echo
  echo "==> Checking the site responds"
  curl -fsS -o /dev/null -w "    https://coursemaster.iambeep.com/ -> %{http_code}\n" https://coursemaster.iambeep.com/ \
    || echo "    WARNING: the site did not respond — check 'docker logs ${CONTAINER}'"
fi
