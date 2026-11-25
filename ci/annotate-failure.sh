#!/usr/bin/env bash
# Usage: ci/annotate-failure.sh <title> <command...>
# Runs the command, keeps its output in the log, and on failure turns the reason into an
# error annotation, which the public check-run annotations API serves without a token.
set -uo pipefail

title=$1
shift
output=$(mktemp)

"$@" 2>&1 | tee "$output"
status=${PIPESTATUS[0]}

if [ "$status" -ne 0 ]; then
  reason=$(awk '/What went wrong:/ {on=1; next} /^\* Try:/ {on=0} on' "$output")
  [ -n "$reason" ] || reason=$(tail -n 40 "$output")
  reason=$(printf '%s' "$reason" | head -c 3000 | sed -e 's/%/%25/g' -e 's/\r/%0D/g' | awk 'BEGIN {ORS="%0A"} {print}')
  printf '::error title=%s::%s\n' "$title" "$reason"
fi

rm -f "$output"
exit "$status"
