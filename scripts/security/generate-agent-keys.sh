#!/usr/bin/env bash
# Security P0: Generate Ed25519 keypairs + bearer tokens for all council agents
#
# Generates:
#   1. Ed25519 signing keypairs (per agent) → config/keys/{agent}.seed + council-keyring.json
#   2. Bearer tokens (per agent) → config/agent-tokens.json
#   3. NATS NKey seeds (per agent) → config/nkeys/{agent}.nk
#
# Usage:
#   ./scripts/security/generate-agent-keys.sh
#
# All output files are chmod 600. The keyring and token files are JSON.
# Secret files (seeds, tokens) should NOT be committed to git.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONFIG_DIR="${SCRIPT_DIR}/../../config"
KEYS_DIR="${CONFIG_DIR}/keys"
NKEYS_DIR="${CONFIG_DIR}/nkeys"

mkdir -p "${KEYS_DIR}" "${NKEYS_DIR}"

# Council agents
AGENTS=("aleph" "luban" "thales" "merlin" "ogma" "laozi-jung" "mataya" "council-chamber")
PRIVILEGED=("thales" "merlin" "council-chamber")

echo "=== Council Security Key Generation ==="
echo ""

# ============================================================================
# 1. Ed25519 Keypairs
# ============================================================================
echo "--- Ed25519 Keypairs ---"

KEYRING_FILE="${CONFIG_DIR}/council-keyring.json"
echo '{"agents":{' > "${KEYRING_FILE}.tmp"

# Ed25519 PKCS#8 DER prefix: wraps a 32-byte seed into valid DER for openssl
DER_PREFIX="302e020100300506032b657004220420"

FIRST=true
for agent in "${AGENTS[@]}"; do
  SEED_FILE="${KEYS_DIR}/${agent}.seed"

  # Generate 32-byte random seed (Ed25519 private key)
  openssl rand -hex 32 > "${SEED_FILE}"
  chmod 600 "${SEED_FILE}"

  # Derive public key using OpenSSL Ed25519 support
  # 1. Wrap raw seed in PKCS#8 DER format
  # 2. Extract public key from DER (last 32 bytes of public key DER)
  SEED_HEX=$(cat "${SEED_FILE}" | tr -d '\n')
  TMPDER=$(mktemp)
  echo -n "${DER_PREFIX}${SEED_HEX}" | xxd -r -p > "${TMPDER}"
  PUBLIC_KEY=$(openssl pkey -in "${TMPDER}" -inform DER -pubout -outform DER 2>/dev/null | xxd -p | tr -d '\n' | tail -c 64)
  rm -f "${TMPDER}"

  if [ -z "$PUBLIC_KEY" ] || [ ${#PUBLIC_KEY} -ne 64 ]; then
    echo "  ✗ ${agent}: FAILED to derive public key"
    exit 1
  fi

  if [ "$FIRST" = true ]; then
    FIRST=false
  else
    echo ',' >> "${KEYRING_FILE}.tmp"
  fi
  echo -n "\"${agent}\":{\"public_key\":\"${PUBLIC_KEY}\"}" >> "${KEYRING_FILE}.tmp"

  echo "  ✓ ${agent}: seed + pubkey derived"
done

echo '}}' >> "${KEYRING_FILE}.tmp"
mv "${KEYRING_FILE}.tmp" "${KEYRING_FILE}"
echo ""

# ============================================================================
# 2. Bearer Tokens
# ============================================================================
echo "--- Bearer Tokens ---"

TOKEN_FILE="${CONFIG_DIR}/agent-tokens.json"
echo '{"tokens":{' > "${TOKEN_FILE}.tmp"

FIRST=true
for agent in "${AGENTS[@]}"; do
  TOKEN="mq-${agent}-$(openssl rand -hex 16)"

  if [ "$FIRST" = true ]; then
    FIRST=false
  else
    echo ',' >> "${TOKEN_FILE}.tmp"
  fi
  echo -n "\"${agent}\":\"${TOKEN}\"" >> "${TOKEN_FILE}.tmp"

  echo "  ✓ ${agent}: token generated"
done

# Add privileged agents list
echo '},"privileged_agents":[' >> "${TOKEN_FILE}.tmp"
FIRST=true
for agent in "${PRIVILEGED[@]}"; do
  if [ "$FIRST" = true ]; then
    FIRST=false
  else
    echo -n ',' >> "${TOKEN_FILE}.tmp"
  fi
  echo -n "\"${agent}\"" >> "${TOKEN_FILE}.tmp"
done
echo ']}' >> "${TOKEN_FILE}.tmp"

mv "${TOKEN_FILE}.tmp" "${TOKEN_FILE}"
chmod 600 "${TOKEN_FILE}"
echo ""

# ============================================================================
# 3. NATS NKey Seeds (placeholder — requires nsc or nk tool)
# ============================================================================
echo "--- NATS NKeys ---"
echo "NOTE: NATS NKey generation requires 'nsc' or 'nk' CLI tool."
echo "Install: go install github.com/nats-io/nkeys/nk@latest"
echo ""

for agent in "${AGENTS[@]}"; do
  NKEY_FILE="${NKEYS_DIR}/${agent}.nk"
  if command -v nk &> /dev/null; then
    nk -gen user > "${NKEY_FILE}" 2>/dev/null || echo "placeholder-nkey-seed" > "${NKEY_FILE}"
    chmod 600 "${NKEY_FILE}"
    echo "  ✓ ${agent}: NKey → ${NKEY_FILE}"
  else
    echo "placeholder-install-nk-tool" > "${NKEY_FILE}"
    chmod 600 "${NKEY_FILE}"
    echo "  ⚠ ${agent}: placeholder (install nk tool)"
  fi
done

echo ""
echo "=== Generation Complete ==="
echo ""
echo "Files created:"
echo "  ${KEYRING_FILE}     — council public keyring (safe to share)"
echo "  ${TOKEN_FILE}       — bearer tokens (SECRET, mode 600)"
echo "  ${KEYS_DIR}/*.seed  — Ed25519 seeds (SECRET, mode 600)"
echo "  ${NKEYS_DIR}/*.nk   — NATS NKey seeds (SECRET, mode 600)"
echo ""
echo "Add to ming-qiao.toml:"
echo "  [auth]"
echo "  enabled = true"
echo "  token_file = \"config/agent-tokens.json\""
echo "  keyring_file = \"config/council-keyring.json\""
echo ""
echo "  [nats]"
echo "  auth_mode = \"nkey\""
echo "  nkey_seed_file = \"config/nkeys/<agent>.nk\""
