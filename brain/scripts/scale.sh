#!/bin/sh
# usage: scale.sh <brain binary> [chunks=1000000] [docs=100000]
#
# The two measurements of the shared index at the size of a big brain, with one released binary
# and nothing else but git (no network, no GitHub: a stand-in `gh` serves the release from a folder):
#
#  1. `brain bench` at <chunks> chunks: search p50 (target <= 500 ms at 1M chunks on a 6-core
#     laptop) and indexing chunks/s with the real model (target >= 12).
#  2. A synthetic brain of <docs> documents: the CI's build (`brain pack`, from scratch), then a
#     fresh machine's first `brain index` on a clone of it, which pulls the published index and
#     embeds nothing (target: minutes, not hours).
#
# Needs about 4 GB of disk per million chunks, and 1 GB for 100k documents; the build of part 2
# embeds every document, so it takes as long as <docs> / chunks-per-second. Everything lives in a
# temporary folder that is removed at the end; nothing under ~ is touched.
set -eu
[ $# -ge 1 ] || { sed -n '2,13p' "$0"; exit 2; }
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
CHUNKS=${2:-1000000}
DOCS=${3:-100000}
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.com GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.com

echo "machine: $(uname -sm), $(getconf _NPROCESSORS_ONLN) cores; binary: $("$BIN" --version)"

echo
echo "== 1. brain bench, $CHUNKS chunks"
SUPERCHARGE_CACHE_DIR=$W/cache-bench "$BIN" bench --chunks "$CHUNKS" --dir "$W"

echo
echo "== 2. a brain of $DOCS documents: the CI's build, then a fresh machine's first index"
"$BIN" bench --write-brain "$W/brain" --docs "$DOCS"

# A stand-in for `gh`: `release view` lists the asset in $W/release, `release download` copies it.
mkdir "$W/bin" "$W/release"
cat > "$W/bin/gh" <<'GH'
#!/bin/sh
if [ "$1 $2 $3" = "release view brain-index" ]; then
  name=$(ls "$FAKE_RELEASE" | grep '^brain-index_' | head -n 1)
  printf '{"assets":[{"name":"%s","createdAt":"2026-01-01T00:00:00Z"}]}\n' "$name"
elif [ "$1 $2 $3" = "release download brain-index" ]; then
  while [ $# -gt 0 ]; do case "$1" in --pattern) p=$2;; --dir) d=$2;; esac; shift; done
  cp "$FAKE_RELEASE/$p" "$d/$p"
else
  echo "unexpected: $*" >&2; exit 1
fi
GH
chmod +x "$W/bin/gh"
export FAKE_RELEASE=$W/release

t0=$(date +%s)
(cd "$W/brain" && SUPERCHARGE_CACHE_DIR=$W/cache-ci "$BIN" pack --out "$W/release" 2>/dev/null)
t1=$(date +%s)
echo "CI build (brain pack, from scratch): $((t1 - t0)) s; asset $(ls -l "$W/release" | awk 'NR==2 {printf "%.0f MB", $5/1e6}')"

git clone -q "$W/brain" "$W/fresh"
git -C "$W/fresh" remote set-url origin https://github.com/bench/brain.git
mkdir "$W/project"
printf '# Project\n' > "$W/project/README.md"
printf '# AGENTS.md\n\n## Supercharge\n\n```yaml\nbrain:\n  - path: %s\n```\n' "$W/fresh" > "$W/project/AGENTS.md"
t0=$(date +%s)
(cd "$W/project" && PATH=$W/bin:$PATH SUPERCHARGE_CACHE_DIR=$W/cache-fresh "$BIN" index 2>/dev/null)
t1=$(date +%s)
echo "fresh machine's first index (pulls the shared index, embeds only what differs): $((t1 - t0)) s"
