#!/bin/sh
# Answers initialize, spawns a grandchild that outlives stdin, and ignores stdin EOF and SIGTERM.
trap '' TERM
sh -c 'trap "" TERM; while :; do sleep 1; done' </dev/null >/dev/null 2>&1 &
echo "grandchild $!" >&2
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      id=$(printf '%s\n' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([^,]*\),.*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"stubborn-fixture","version":"1.0.0"}}}\n' "$id" ;;
  esac
done
while :; do sleep 1; done
