#!/bin/sh
# Newline-delimited JSON-RPC MCP server for tests. Relies on the client's compact key order
# ({"jsonrpc":"2.0","id":...,"method":...}).
echo "stdio fixture ready" >&2
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([^,]*\),.*/\1/p')
  [ -z "$id" ] && continue
  method=$(printf '%s\n' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  case "$method" in
    initialize)
      result='{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"stdio-fixture","version":"1.0.0"}}' ;;
    tools/list)
      result='{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}' ;;
    tools/call)
      text=$(printf '%s\n' "$line" | sed -n 's/.*"text":"\([^"]*\)".*/\1/p')
      result="{\"content\":[{\"type\":\"text\",\"text\":\"$text\"}]}" ;;
    ping)
      result='{}' ;;
    *)
      printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"not found"}}\n' "$id"
      continue ;;
  esac
  printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$id" "$result"
done
