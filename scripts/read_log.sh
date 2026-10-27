#!/bin/bash

log_path="$1"

jq -r '
  "\(.timestamp) \(.level) \(.fields.message // "") " +
  ((.fields | del(.message) | to_entries
    | map("\(.key)=\(.value | tostring)") | join(" ")))
' "$log_path"
