#!/usr/bin/env bash

# Run pre-commit and capture both output and exit code
output=$(pre-commit run "$@" 2>&1)
exit_code=$?

# Print output to stderr
echo "$output" >&2

# Convert exit code 1 to 2, keep others as-is
if [ $exit_code -eq 1 ]; then
  exit 2
else
  exit $exit_code
fi
