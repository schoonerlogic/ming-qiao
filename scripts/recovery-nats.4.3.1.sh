#! /bin/bash

echo "=== NATS config in agent TOMLs ==="
for toml in \
  /Users/proteus/astralmaris/ming-qiao/aleph/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-thales.toml \
  /Users/proteus/astralmaris/ming-qiao/luban/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/mataya/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-ogma.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-laozi-jung.toml \
  /Users/proteus/astralmaris/ming-qiao/merlin/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-council-chamber.toml \
; do
  name=$(basename $toml .toml)
  nats_enabled=$(grep -A1 '^\[nats\]' "$toml" 2>/dev/null | grep 'enabled' | grep -o 'true\|false' || echo "MISSING")
  auth_mode=$(grep 'auth_mode' "$toml" 2>/dev/null | grep -o '"[^"]*"' || echo "MISSING")
  nkey_file=$(grep 'nkey_seed_file' "$toml" 2>/dev/null | grep -o '"[^"]*"' || echo "MISSING")
  echo "  $name: nats=$nats_enabled auth=$auth_mode nkey=$nkey_file"
done
