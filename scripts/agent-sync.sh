#!/bin/bash
# sync-worktrees.sh
INTEGRATION_BRANCH="develop"  # adjust to your actual integration branch

SKIP_BRANCHES="develop agent/proteus/nats-evolution"

for wt in $(git worktree list --porcelain | grep "^worktree " | cut -d' ' -f2); do
  # Skip bare repos
  if git -C "$wt" rev-parse --is-bare-repository 2>/dev/null | grep -q true; then
    echo "--- Skipping bare repo: $wt ---"
    continue
  fi

  if echo "$SKIP_BRANCHES" | grep -qw "$branch"; then
    echo "--- Skipping operator branch: $branch in $wt ---"
    continue
  fi

  branch=$(git -C "$wt" branch --show-current)
  
  # Don't rebase the integration branch onto itself
  if [ "$branch" = "$INTEGRATION_BRANCH" ]; then
    echo "--- Pulling latest for integration branch in: $wt ---"
    git -C "$wt" pull --ff-only origin "$INTEGRATION_BRANCH"
    continue
  fi

  echo "--- Rebasing $branch onto $INTEGRATION_BRANCH in: $wt ---"
  git -C "$wt" fetch origin
  git -C "$wt" rebase origin/$INTEGRATION_BRANCH
done
