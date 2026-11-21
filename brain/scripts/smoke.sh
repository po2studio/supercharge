#!/bin/sh
# A release binary on its own, with an empty environment and nothing else around it, indexes and
# searches (feature 0002, slice 1, AC 7). Usage: smoke.sh path/to/brain
set -eu
bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/brain"
printf '# Animals\n\nAnimals are not permitted on the premises except certified assistance canines.\n' > "$work/brain/animals.md"
printf 'Invoice INV-2024-0042 was paid on 3 March, but INV-2024-0043 is still open.' > "$work/brain/invoices.txt"
run() { (cd "$work" && env -i SUPERCHARGE_CACHE_DIR="$work/cache" "$bin" --root "$work/brain" "$@"); }
run index
run search --terms INV-2024-0042 --json | grep -q invoices.txt
run search --about "Can I bring my dog to work?" --json | grep -q animals.md
run stats | grep -q "2 files"
echo "smoke ok: $bin"
