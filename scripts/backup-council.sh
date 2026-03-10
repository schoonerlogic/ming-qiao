#!/usr/bin/env bash
# backup-council.sh — Backup SurrealDB and FalkorDB data
# Safe to run from cron daily.
#
# Usage: backup-council.sh [--dry-run]
#
# Backups go to ~/astralmaris/backups/{surrealdb,falkordb}/YYYY-MM-DD.*

set -euo pipefail

BACKUP_ROOT="${HOME}/astralmaris/backups"
DATE=$(date +%Y-%m-%d)
DRY_RUN=false

if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
    echo "[DRY RUN] Would create backups for ${DATE}"
fi

SURREAL_DIR="${BACKUP_ROOT}/surrealdb"
FALKOR_DIR="${BACKUP_ROOT}/falkordb"

# Create directory structure
mkdir -p "${SURREAL_DIR}" "${FALKOR_DIR}"

ERRORS=0

# --- SurrealDB Export ---
echo "=== SurrealDB Backup ==="
SURREAL_FILE="${SURREAL_DIR}/${DATE}.surql"

if [[ "${DRY_RUN}" == true ]]; then
    echo "  Would export to: ${SURREAL_FILE}"
else
    if curl -sf http://localhost:8000/health > /dev/null 2>&1; then
        if surreal export \
            -e http://localhost:8000 \
            -u root -p root \
            --ns astralmaris --db mingqiao \
            "${SURREAL_FILE}" 2>/dev/null; then
            SIZE=$(du -h "${SURREAL_FILE}" | cut -f1)
            echo "  OK: ${SURREAL_FILE} (${SIZE})"
        else
            echo "  FAIL: surreal export failed"
            ERRORS=$((ERRORS + 1))
        fi
    else
        echo "  FAIL: SurrealDB not reachable on localhost:8000"
        ERRORS=$((ERRORS + 1))
    fi
fi

# --- FalkorDB Backup ---
echo "=== FalkorDB Backup ==="
FALKOR_FILE="${FALKOR_DIR}/${DATE}.rdb"

if [[ "${DRY_RUN}" == true ]]; then
    echo "  Would save to: ${FALKOR_FILE}"
else
    CONTAINER="docker-falkordb-1"
    if docker ps --format '{{.Names}}' | grep -q "^${CONTAINER}$"; then
        # Trigger background save
        docker exec "${CONTAINER}" redis-cli -p 6379 BGSAVE > /dev/null 2>&1
        # Wait for save to complete (up to 30s)
        for i in $(seq 1 30); do
            SAVE_STATUS=$(docker exec "${CONTAINER}" redis-cli -p 6379 LASTSAVE 2>/dev/null)
            sleep 1
            NEW_STATUS=$(docker exec "${CONTAINER}" redis-cli -p 6379 LASTSAVE 2>/dev/null)
            if [[ "${SAVE_STATUS}" != "${NEW_STATUS}" ]] || [[ "${i}" -gt 2 ]]; then
                break
            fi
        done
        # Copy dump.rdb from container
        if docker cp "${CONTAINER}:/var/lib/falkordb/data/dump.rdb" "${FALKOR_FILE}" 2>/dev/null; then
            SIZE=$(du -h "${FALKOR_FILE}" | cut -f1)
            echo "  OK: ${FALKOR_FILE} (${SIZE})"
        else
            echo "  FAIL: could not copy dump.rdb from container"
            ERRORS=$((ERRORS + 1))
        fi
    else
        echo "  FAIL: Container ${CONTAINER} not running"
        ERRORS=$((ERRORS + 1))
    fi
fi

# --- Cleanup old backups (keep 14 days) ---
echo "=== Cleanup ==="
if [[ "${DRY_RUN}" == true ]]; then
    echo "  Would remove backups older than 14 days"
else
    DELETED=0
    for dir in "${SURREAL_DIR}" "${FALKOR_DIR}"; do
        while IFS= read -r -d '' old_file; do
            rm -f "${old_file}"
            DELETED=$((DELETED + 1))
        done < <(find "${dir}" -type f -mtime +14 -print0 2>/dev/null)
    done
    echo "  Removed ${DELETED} old backup(s)"
fi

# --- Summary ---
echo "=== Done ==="
if [[ "${ERRORS}" -gt 0 ]]; then
    echo "  ${ERRORS} error(s) occurred"
    exit 1
else
    echo "  All backups successful"
    exit 0
fi
