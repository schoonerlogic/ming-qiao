#!/usr/bin/env bash
# Security P0: SurrealDB Credential Rotation
#
# Creates a database-scoped user for ming-qiao, replacing root/root for runtime use.
# The root credentials are only used by this script to create the service user.
#
# Usage:
#   ./scripts/security/setup-surreal-creds.sh
#
# Outputs:
#   - Prints MINGQIAO_DB_USERNAME and MINGQIAO_DB_PASSWORD env vars to set
#   - Saves credentials to config/surreal-creds.env (chmod 600)

set -euo pipefail

SURREAL_URL="${SURREAL_URL:-http://localhost:8000}"
SURREAL_ROOT_USER="${SURREAL_ROOT_USER:-root}"
SURREAL_ROOT_PASS="${SURREAL_ROOT_PASS:?Set SURREAL_ROOT_PASS or source config/surreal-root.env}"
NAMESPACE="astralmaris"
DATABASE="mingqiao"
SERVICE_USER="mingqiao_service"

# Generate a random password (32 chars, alphanumeric)
SERVICE_PASS=$(openssl rand -base64 32 | tr -dc 'a-zA-Z0-9' | head -c 32)

echo "=== SurrealDB Credential Rotation ==="
echo "Server: ${SURREAL_URL}"
echo "Namespace: ${NAMESPACE}"
echo "Database: ${DATABASE}"
echo "Service user: ${SERVICE_USER}"
echo ""

# Create (or update) the database-scoped user via SurrealDB HTTP API
# DEFINE USER OVERWRITE ensures password is rotated even if user already exists
# The user gets OWNER-level access to the database (can create tables, indexes)
QUERY="DEFINE USER OVERWRITE ${SERVICE_USER} ON DATABASE PASSWORD '${SERVICE_PASS}' ROLES OWNER;"

echo "Creating database-scoped user..."
RESPONSE=$(curl -s -w "\n%{http_code}" \
  "${SURREAL_URL}/sql" \
  -H "Accept: application/json" \
  -H "surreal-ns: ${NAMESPACE}" \
  -H "surreal-db: ${DATABASE}" \
  --user "${SURREAL_ROOT_USER}:${SURREAL_ROOT_PASS}" \
  --data-raw "${QUERY}" 2>&1)

HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | sed '$d')

if [ "$HTTP_CODE" = "200" ]; then
  # Check for SurrealDB-level errors in the JSON response
  if echo "$BODY" | grep -q '"status":"ERR"'; then
    echo "✗ SurrealDB error:"
    echo "$BODY"
    exit 1
  fi
  echo "✓ Database user '${SERVICE_USER}' created successfully"
else
  echo "✗ Failed to create user (HTTP ${HTTP_CODE})"
  echo "Response: ${BODY}"
  exit 1
fi

# Verify the new credentials work via HTTP basic auth with db-scoped headers
echo "Verifying new credentials..."
VERIFY_RESPONSE=$(curl -s -w "\n%{http_code}" \
  "${SURREAL_URL}/sql" \
  -H "Accept: application/json" \
  -H "surreal-ns: ${NAMESPACE}" \
  -H "surreal-db: ${DATABASE}" \
  --user "${SERVICE_USER}:${SERVICE_PASS}" \
  --data-raw "SELECT count() FROM event GROUP ALL;" 2>&1)

VERIFY_CODE=$(echo "$VERIFY_RESPONSE" | tail -n1)

if [ "$VERIFY_CODE" = "200" ]; then
  echo "✓ New credentials verified via HTTP"
else
  echo "⚠ HTTP verification returned ${VERIFY_CODE} (DB-scoped users may require signin — ws:// will be tested at runtime)"
fi

# Save credentials
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CREDS_DIR="${SCRIPT_DIR}/../../config"
mkdir -p "${CREDS_DIR}"
CREDS_FILE="${CREDS_DIR}/surreal-creds.env"

cat > "${CREDS_FILE}" << EOF
# SurrealDB credentials for ming-qiao (Security P0)
# Generated: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
# Auth level: database (scoped to ${NAMESPACE}/${DATABASE})
# DO NOT commit this file — it is in .gitignore
export MINGQIAO_DB_USERNAME="${SERVICE_USER}"
export MINGQIAO_DB_PASSWORD="${SERVICE_PASS}"
EOF

chmod 600 "${CREDS_FILE}"

echo ""
echo "=== Credentials saved ==="
echo "File: ${CREDS_FILE} (mode 600)"
echo ""
echo "To use:"
echo "  source ${CREDS_FILE}"
echo "  # Then update ming-qiao.toml:"
echo "  #   [database]"
echo "  #   auth_level = \"database\""
echo "  #   # username/password read from env vars automatically"
echo ""
echo "Environment variables:"
echo "  MINGQIAO_DB_USERNAME=${SERVICE_USER}"
echo "  MINGQIAO_DB_PASSWORD=<generated>"
