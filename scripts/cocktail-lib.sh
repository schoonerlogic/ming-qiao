#!/bin/bash
# cocktail-lib.sh — Shared security functions for cocktail-party hooks
# Implements: path hardening (RA-004), atomic writes (RA-008), token stripping
#
# Source this file from cocktail scripts:
#   source "$(dirname "$0")/cocktail-lib.sh"

# ── Path Hardening ──

# Known agent base path — all agent worktrees live under this directory
MING_QIAO_BASE="/Users/proteus/astralmaris/ming-qiao"

# Resolve agent ID from CWD with path hardening.
# Rejects paths with ".." components, resolves symlinks via realpath,
# and matches against known base paths (not substrings).
#
# Usage: resolve_agent_id "$CWD"
# Sets: AGENT variable, or returns 1 if CWD doesn't match a known agent.
resolve_agent_id() {
    local raw_cwd="$1"

    # Reject paths containing ".." components
    if [[ "$raw_cwd" == *".."* ]]; then
        return 1
    fi

    # Canonicalize path (resolve symlinks, normalize)
    local canon_cwd
    canon_cwd=$(realpath -q "$raw_cwd" 2>/dev/null) || return 1

    # Verify the canonicalized path is under the known base
    if [[ "$canon_cwd" != "$MING_QIAO_BASE/"* ]]; then
        return 1
    fi

    # Extract the agent directory name (first path component after base)
    local relative="${canon_cwd#$MING_QIAO_BASE/}"
    local agent_dir="${relative%%/*}"

    # Validate against known agent IDs
    case "$agent_dir" in
        aleph|luban|merlin|thales|ogma|laozi-jung|mataya)
            AGENT="$agent_dir"
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

# ── Atomic File Writes ──

# Write content to a file atomically using temp-file-then-rename.
# Creates temp file in the same directory (same filesystem) for atomic rename.
# Sets restrictive permissions (600) on temp file BEFORE rename.
#
# Usage: atomic_write "target_path" "content"
atomic_write() {
    local target="$1"
    local content="$2"
    local target_dir
    target_dir=$(dirname "$target")

    # Ensure target directory exists
    mkdir -p "$target_dir"

    # Create temp file in same directory (same filesystem = atomic rename)
    local tmpfile
    tmpfile=$(mktemp "${target_dir}/.tmp.XXXXXX") || return 1

    # Set restrictive permissions BEFORE writing content
    chmod 600 "$tmpfile"

    # Write content
    printf '%s' "$content" > "$tmpfile" || { rm -f "$tmpfile"; return 1; }

    # Atomic rename
    mv "$tmpfile" "$target" || { rm -f "$tmpfile"; return 1; }
}

# ── Token Stripping ──

# Strip bearer token patterns from text content before display or persistence.
# Catches: "Bearer mq-*", "Authorization: Bearer *", raw "mq-<agent>-<hex>" tokens
#
# Usage: cleaned=$(strip_tokens "$text")
strip_tokens() {
    local text="$1"
    # Strip "Authorization: Bearer <token>" headers
    text=$(echo "$text" | sed -E 's/Authorization: Bearer [^ "]+/Authorization: Bearer [REDACTED]/g')
    # Strip raw "mq-<agent>-<hex>" token patterns
    text=$(echo "$text" | sed -E 's/mq-[a-z]+-[0-9a-f]{32}/[TOKEN-REDACTED]/g')
    # Strip "Bearer <token>" standalone
    text=$(echo "$text" | sed -E 's/Bearer mq-[a-z]+-[0-9a-f]{32}/Bearer [REDACTED]/g')
    echo "$text"
}
