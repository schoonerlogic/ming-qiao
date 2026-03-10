#!/usr/bin/env bash
# Source this file to set up Merlin's terminal environment
# Usage: source scripts/merlin-env.sh  (or add to .zshrc)

export MERLIN_HOME="/Users/proteus/astralmaris/ming-qiao/merlin"
export PATH="$MERLIN_HOME/scripts:$PATH"

# ORACLE convenience
export ORACLE_MCP="http://localhost:8001/mcp"
export ORACLE_HOME="/Users/proteus/astralmaris/oracle"

# Aliases
alias mq='council'
alias mq-threads='council threads'
alias mq-inbox='council inbox'
alias mq-agents='council agents'
alias mq-recent='council recent'
alias mq-status='council status'

alias ora='oracle'
alias ora-status='oracle status'
alias ora-ingest='oracle ingest'

echo "Merlin environment loaded."
echo "  council <cmd>   — Council communication"
echo "  oracle <cmd>    — ORACLE knowledge graph"
echo "  mq / ora        — short aliases"
