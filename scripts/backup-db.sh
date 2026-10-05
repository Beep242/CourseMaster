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
#   CM_HELPER_IMAGE      image providing sqlite3         (default alpine:latest)

set -euo pipefail

SSH_TARGET="${CM_SSH_TARGET:-root@95.216.166.108}"
CONTAINER="${CM_CONTAINER:-coursemaster-api-1}"
DB_PATH="${CM_DB_PATH:-/app/data/coursemaster.db}"
BACKUP_DIR="${CM_BACKUP_DIR:-/opt/coursemaster/backups}"
LOCAL_BACKUP_DIR="${CM_LOCAL_BACKUP_DIR:-./backups}"
HELPER_IMAGE="${CM_HELPER_IMAGE:-alpine:latest}"

STAMP="$(date -u +%Y%m%d-%H%M%S)"
NAME="coursemaster-${STAMP}.db"

echo "==> Snapshotting ${CONTAINER}:${DB_PATH}"

# `sqlite3 .backup` is the only correct way to copy a live SQLite database: a
# plain `cp` races with in-flight writes and, with WAL enabled, would also miss
# the `-wal` sidecar holding the most recent commits.
#
# It runs in a THROWAWAY ALPINE CONTAINER mounting the same data directory,
# rather than installing sqlite3 into the app container. The app image ships no
# sqlite3, an install there does not survive a container recreate, and — the
# reason this changed — apt inside that image now fails outright with
# "At least one invalid signature was encountered" on the Debian security and
# nodesource repos, so the install cannot succeed at all. `.backup` from a
# separate process is safe: it uses SQLite's online backup API and takes a read
# lock, so the running app is not interrupted.
ssh "$SSH_TARGET" bash -s -- "$CONTAINER" "$DB_PATH" "$BACKUP_DIR" "$NAME" "$HELPER_IMAGE" <<'REMOTE'
set -euo pipefail
CONTAINER="$1"; DB_PATH="$2"; BACKUP_DIR="$3"; NAME="$4"; HELPER_IMAGE="$5"
DATA_DIR="$(dirname "$DB_PATH")"
DB_FILE="$(basename "$DB_PATH")"

if [ -z "$(docker ps -q -f "name=^${CONTAINER}$")" ]; then
  echo "FATAL: container ${CONTAINER} is not running — nothing to snapshot." >&2
  exit 1
fi

# `.Source` is the host path for a bind mount AND for a named volume (the
# latter under /var/lib/docker/volumes/<name>/_data), so one lookup covers both.
HOST_DATA_DIR="$(docker inspect "$CONTAINER" \
  --format '{{range .Mounts}}{{if eq .Destination "'"$DATA_DIR"'"}}{{.Source}}{{end}}{{end}}')"
if [ -z "$HOST_DATA_DIR" ] || [ ! -d "$HOST_DATA_DIR" ]; then
  echo "FATAL: could not resolve the host path for ${DATA_DIR} in ${CONTAINER}." >&2
  exit 1
fi

docker run --rm -v "${HOST_DATA_DIR}:/data" "$HELPER_IMAGE" sh -c "
  set -e
  apk add --no-cache sqlite >/dev/null 2>&1
  sqlite3 '/data/${DB_FILE}' \".backup '/data/.snapshot.db'\"
  CHECK=\$(sqlite3 /data/.snapshot.db 'PRAGMA integrity_check;')
  if [ \"\$CHECK\" != 'ok' ]; then
    echo \"FATAL: snapshot failed integrity_check: \$CHECK\" >&2
    rm -f /data/.snapshot.db
    exit 1
  fi
  echo '    integrity_check: ok'
  # A snapshot nobody can describe is a snapshot nobody trusts — print what is
  # in it so a restore decision does not need a running app.
  sqlite3 /data/.snapshot.db \"
    SELECT '    ' || 'courses='   || (SELECT COUNT(*) FROM courses)
        || ' assignments='        || (SELECT COUNT(*) FROM assignments)
        || ' extractions='        || (SELECT COUNT(*) FROM syllabus_extractions)
        || ' decks='              || (SELECT COUNT(*) FROM decks)
        || ' cards='              || (SELECT COUNT(*) FROM cards)
        || ' tables='             || (SELECT COUNT(*) FROM sqlite_master WHERE type='table')
        || ' schema_version='     || (SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations);\"
"

mkdir -p "$BACKUP_DIR"
mv "${HOST_DATA_DIR}/.snapshot.db" "${BACKUP_DIR}/${NAME}"
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
