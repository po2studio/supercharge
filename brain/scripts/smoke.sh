#!/bin/sh
# A release binary on its own, with an empty environment and nothing else around it, indexes and
# searches a project and a folder its AGENTS.md lists, with no flags (feature 0002, slices 1 and
# 1b). Usage: smoke.sh path/to/brain
set -eu
bin=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/repo/docs" "$work/policies"
printf '# Animals\n\nAnimals are not permitted on the premises except certified assistance canines.\n' > "$work/repo/docs/animals.md"
printf 'Invoice INV-2024-0042 was paid on 3 March, but INV-2024-0043 is still open.' > "$work/policies/invoices.txt"
printf '# AGENTS.md\n\n## Supercharge\n\n```yaml\nbrain:\n  - path: ../policies\n```\n' > "$work/repo/AGENTS.md"
run() { (cd "$work/repo" && env -i SUPERCHARGE_CACHE_DIR="$work/cache" "$bin" "$@"); }
run index
run search --terms INV-2024-0042 --json | grep -q invoices.txt
run search --about "Can I bring my dog to work?" --json | grep -q animals.md
run stats | grep -q "indexed  2 files"
printf '# Brain\n' > "$work/policies/BRAIN.md"
run --repo "$work/policies" check | grep -q "0 failures"
echo "smoke ok: $bin"
