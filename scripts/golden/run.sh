#!/usr/bin/env bash
# Regenerate the golden UI fixtures from the TypeScript reference.
#
#   PI_TS_REF=/path/to/pi scripts/golden/run.sh [gen-*.mjs ...]
#
# The reference checkout needs its dependencies installed (`npm ci --ignore-scripts` at its root).
# Requires Node >= 22.6 (type stripping). Fixtures are written to
# crates/pi-coding-agent/tests/fixtures/golden/.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
: "${PI_TS_REF:?set PI_TS_REF to the TypeScript pi checkout}"
export PI_TS_REF
# Fixed environment: the Rust tests use the same values (see golden_ui.rs).
export HOME=/home/golden
export TZ=UTC
export LANG=C.UTF-8
export PI_OFFLINE=1
unset NO_COLOR
export FORCE_COLOR=3
export COLORTERM=truecolor
export TERM=xterm-256color
unset TERM_PROGRAM KITTY_WINDOW_ID WEZTERM_PANE GHOSTTY_RESOURCES_DIR ITERM_SESSION_ID TMUX STY 2>/dev/null || true
export PI_CODING_AGENT_DIR=/home/golden/.pi/agent

# The reference's model catalog data is generated (network); reuse the copy the Rust port embeds.
data_dir="$PI_TS_REF/packages/ai/src/providers/data"
if [ ! -f "$data_dir/.manifest.json" ]; then
	mkdir -p "$data_dir"
	cp -R "$here/../../crates/pi-ai/data/." "$data_dir/"
fi

scripts=()
for s in "$@"; do scripts+=("$(cd "$(dirname "$s")" && pwd)/$(basename "$s")"); done
if [ ${#scripts[@]} -eq 0 ]; then
	scripts=("$here"/gen-*.mjs)
fi
cd "$here"
for s in "${scripts[@]}"; do
	node --experimental-strip-types --no-warnings --import "$here/register.mjs" "$s"
done
