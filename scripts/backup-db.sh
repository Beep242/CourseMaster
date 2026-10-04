#!/usr/bin/env bash
#
# Snapshot the production CourseMaster SQLite database, verify the snapshot,
# and pull a copy down to this machine.
#
# Run this BEFORE every schema-changing deploy. There is no other copy of this
# data and no working image rollback: once a migration applies, an older
# binary boot-panics on sqlx's `VersionMissing` check, and because that same
# binary also serves the frontend, the whole site goes down with it. A verified
# snapshot is the only way back.
#
#   ./scripts/backup-db.sh
#
# Environment overrides (all optional):
#   CM_SSH_TARGET        ssh destination                 (default root@95.216.166.108)
#   CM_CONTAINER         container name                  (default coursemaster-api-1)
#   CM_DB_PATH           db path inside the container    (default /app/data/coursemaster.db)
#   CM_BACKUP_DIR        snapshot dir on the VPS         (default /opt/coursemaster/backups)
#   CM_LOCAL_BACKUP_DIR  snapshot dir on this machine    (default ./backups)

set -euo pipefail

SSH_TARGET="${CM_SSH_TARGET:-root@95.216.166.108}"
CONTAINER="${CM_CONTAINER:-coursemaster-api-1}"
DB_PATH="${CM_DB_PATH:-/app/data/coursemaster.db}"
BACKUP_DIR="${CM_BACKUP_DIR:-/opt/coursemaster/backups}"
LOCAL_BACKUP_DIR="${CM_LOCAL_BACKUP_DIR:-./backups}"

STAMP="$(date -u +%Y%m%d-%H%M%S)"
NAME="coursemaster-${STAMP}.db"
# Staged inside the container's data volume, because `docker cp` can only read
# paths the container can see.
STAGED="$(dirname "$DB_PATH")/.backup-${STAMP}.db"

echo "==> Snapshotting ${CONTAINER}:${DB_PATH}"

# `sqlite3 .backup` is the only correct way to copy a live SQLite database: a
# plain `cp` of the file races with in-flight writes, and once WAL is enabled it
# would also silently miss the -wal sidecar. sqlite3 is not in the runtime image
# (the Dockerfile installs only Node and the Claude CLI) and an `apt-get install`
# does not survive a container recreate, so it is installed on demand each run.
ssh "$SSH_TARGET" bash -s -- "$CONTAINER" "$DB_PATH" "$STAGED" "$BACKUP_DIR" "$NAME" <<'REMOTE'
set -euo pipefail
CONTAINER="$1"; DB_PATH="$2"; STAGED="$3"; BACKUP_DIR="$4"; NAME="$5"

if [ -z "$(docker ps -q -f "name=^${CONTAINER}$")" ]; then
  echo "FATAL: container ${CONTAINER} is not running — nothing to snapshot." >&2
  exit 1
fi

docker exec "$CONTAINER" sh -c '
  set -e
  command -v sqlite3 >/dev/null 2>&1 || {
    echo "    (installing sqlite3 in the container — not baked into the image)"
    apt-get update -qq >/dev/null 2>&1
    apt-get install -y -qq sqlite3 >/dev/null 2>&1
  }
'

docker exec "$CONTAINER" sqlite3 "$DB_PATH" ".backup '${STAGED}'"

CHECK="$(docker exec "$CONTAINER" sqlite3 "$STAGED" 'PRAGMA integrity_check;')"
if [ "$CHECK" != "ok" ]; then
  echo "FATAL: snapshot failed integrity_check: ${CHECK}" >&2
  docker exec "$CONTAINER" rm -f "$STAGED" || true
  exit 1
fi
echo "    integrity_check: ok"

# A snapshot nobody can describe is a snapshot nobody trusts — record what is
# in it next to it, so a restore decision does not need a running app.
docker exec "$CONTAINER" sqlite3 "$STAGED" "
  SELECT 'courses='        || (SELECT COUNT(*) FROM courses)
      || ' assignments='   || (SELECT COUNT(*) FROM assignments)
      || ' extractions='   || (SELECT COUNT(*) FROM syllabus_extractions)
      || ' feeds='         || (SELECT COUNT(*) FROM calendar_feeds)
      || ' tables='        || (SELECT COUNT(*) FROM sqlite_master WHERE type='table')
      || ' schema_version='|| (SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations);
"

mkdir -p "$BACKUP_DIR"
docker cp "${CONTAINER}:${STAGED}" "${BACKUP_DIR}/${NAME}"
docker exec "$CONTAINER" rm -f "$STAGED"
echo "    on VPS: ${BACKUP_DIR}/${NAME} ($(stat -c %s "${BACKUP_DIR}/${NAME}") bytes)"
REMOTE

echo "==> Pulling a copy to this machine"
mkdir -p "$LOCAL_BACKUP_DIR"
scp -q "${SSH_TARGET}:${BACKUP_DIR}/${NAME}" "${LOCAL_BACKUP_DIR}/${NAME}"

if command -v sqlite3 >/dev/null 2>&1; then
  LOCAL_CHECK="$(sqlite3 "${LOCAL_BACKUP_DIR}/${NAME}" 'PRAGMA integrity_check;')"
  if [ "$LOCAL_CHECK" != "ok" ]; then
    echo "FATAL: the local copy failed integrity_check: ${LOCAL_CHECK}" >&2
    exit 1
  fi
  echo "    local copy verified: ${LOCAL_BACKUP_DIR}/${NAME}"
else
  echo "    local copy: ${LOCAL_BACKUP_DIR}/${NAME} (no local sqlite3; verified on the VPS only)"
fi

echo
echo "Done. To roll this back:"
echo "  ./scripts/restore-db.sh ${NAME}"
