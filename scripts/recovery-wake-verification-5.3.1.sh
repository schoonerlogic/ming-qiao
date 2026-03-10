#! /bin/bash

# Check the last message from an agent (should see a recent timestamp)
curl -s "http://localhost:7777/api/inbox/aleph?limit=1" | python3 -c "
import sys, json
data = json.load(sys.stdin)
msgs = data.get('messages', [])
if msgs:
    print(f'Latest: {msgs[0][\"timestamp\"][:19]} from {msgs[0][\"from\"]}: {msgs[0][\"subject\"][:50]}')
else:
    print('No messages yet')
"
