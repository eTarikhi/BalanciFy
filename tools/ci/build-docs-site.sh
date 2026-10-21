#!/bin/sh
# Assemble a self-contained Pages artifact from reviewed static source files.
set -eu

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
  echo "usage: $0 OUTPUT_DIRECTORY" >&2
  exit 2
fi

output=$1
case "$output" in
  /*) ;;
  *) output="$(pwd)/$output" ;;
esac

mkdir -p "$output/assets/progress"
cp docs-site/index.html docs-site/404.html "$output/"
cp docs/assets/progress/*2026-10-22.svg "$output/assets/progress/"
: > "$output/.nojekyll"

# Guard the artifact contract so an accidental root-path change fails CI.
test -s "$output/index.html"
test -s "$output/404.html"
test -s "$output/assets/progress/status-dashboard-2026-10-22.svg"
