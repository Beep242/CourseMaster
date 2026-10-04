#!/usr/bin/env bash
#
# Restore the production CourseMaster database from a snapshot taken by
# ./scripts/backup-db.sh. DESTRUCTIVE: the current database is replaced.
#
#   ./scripts/restore-db.sh coursemaster-20261002-201433.db
#
# The snapshot is read from CM_BACKUP_DIR on the VPS. The container is stopped
# for the swap rather than being written underneath — replacing the file of a
# live SQLite connection corrupts it, and with WAL enabled would leave a -wal
# sidecar describing a database that no longer exists.
#
# Before the swap the CURRENT database is itself snapshotted, so a restore run
# against the wrong file is also recoverable.
#
# Environment overrides match backup-db.sh:
#   CM_SSH_TARGET  CM_CONTAINER  CM_DB_PATH  CM_BACKUP_DIR

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

[ -f "$SRC" ] || { echo "FATAL: no such snapshot: ${SRC}" >&2; exit 1; }

# Never restore a snapshot that cannot be read — that would trade a working
# database for a broken one.
if command -v sqlite3 >/dev/null 2>&1; then
  CHECK="$(sqlite3 "$SRC" 'PRAGMA integrity_check;')"
  [ "$CHECK" = "ok" ] || { echo "FATAL: snapshot is corrupt: ${CHECK}" >&2; exit 1; }
  echo "==> snapshot integrity_check: ok"
else
  echo "==> WARNING: no sqlite3 on the VPS host; restoring an unverified snapshot"
fi

SAFETY="${BACKUP_DIR}/pre-restore-$(date -u +%Y%m%d-%H%M%S).db"
echo "==> Snapshotting the CURRENT database first -> ${SAFETY}"
docker exec "$CONTAINER" sh -c 'command -v sqlite3 >/dev/null 2>&1 || { apt-get update -qq >/dev/null 2>&1; apt-get install -y -qq sqlite3 >/dev/null 2>&1; }'
docker exec "$CONTAINER" sqlite3 "$DB_PATH" ".backup '/tmp/pre-restore.db'"
docker cp "${CONTAINER}:/tmp/pre-restore.db" "$SAFETY"
docker exec "$CONTAINER" rm -f /tmp/pre-restore.db

echo "==> Stopping ${CONTAINER}"
docker stop "$CONTAINER" >/dev/null

echo "==> Swapping in the snapshot"
docker cp "$SRC" "${CONTAINER}:${DB_PATH}"
# WAL/SHM sidecars describe the OLD database; leaving them would be read as
# pending transactions against a file that is no longer there.
docker exec "$CONTAINER" sh -c "rm -f '${DB_PATH}-wal' '${DB_PATH}-shm'" 2>/dev/null || true

echo "==> Starting ${CONTAINER}"
docker start "$CONTAINER" >/dev/null
sleep 3

echo "==> Post-restore state"
docker exec "$CONTAINER" sh -c 'command -v sqlite3 >/dev/null 2>&1 || { apt-get update -qq >/dev/null 2>&1; apt-get install -y -qq sqlite3 >/dev/null 2>&1; }'
docker exec "$CONTAINER" sqlite3 "$DB_PATH" "
  SELECT 'courses='        || (SELECT COUNT(*) FROM courses)
      || ' assignments='   || (SELECT COUNT(*) FROM assignments)
      || ' extractions='   || (SELECT COUNT(*) FROM syllabus_extractions)
      || ' schema_version='|| (SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations);
"
docker compose -f /opt/coursemaster/docker-compose.prod.yml ps --format '{{.Name}} {{.State}}' 2>/dev/null || docker ps --filter "name=${CONTAINER}" --format '{{.Names}} {{.State}}'
echo "    safety copy of the replaced database: ${SAFETY}"
REMOTE

echo
echo "==> Checking the site responds"
curl -fsS -o /dev/null -w "    https://coursemaster.iambeep.com/ -> %{http_code}\n" https://coursemaster.iambeep.com/ \
  || echo "    WARNING: the site did not respond — check 'docker logs ${CONTAINER}'"
