#!/usr/bin/env bash
# List remaining `todo!("port ...")` markers (porting work items), grouped by file with counts.
#
# Usage: scripts/port-todo.sh [-v] [path ...]
#   -v      also print each marker (line number and TS symbol)
#   path    files or directories to scan (default: crates/)
set -euo pipefail

verbose=0
if [[ "${1:-}" == "-v" ]]; then
	verbose=1
	shift
fi

cd "$(dirname "$0")/.."
paths=("$@")
if [[ ${#paths[@]} -eq 0 ]]; then
	paths=(crates)
fi

pattern='todo!\("port '
matches=$(grep -rnE --include='*.rs' "$pattern" "${paths[@]}" || true)
if [[ -z "$matches" ]]; then
	echo "No port markers left."
	exit 0
fi

echo "$matches" | awk -F: -v verbose="$verbose" '
	{
		file = $1
		count[file]++
		total++
		if (verbose) {
			line = $0
			sub(/^[^:]*:/, "", line)
			lineno = line
			sub(/:.*/, "", lineno)
			sym = line
			sub(/.*todo!\("port /, "", sym)
			sub(/"\).*/, "", sym)
			items[file] = items[file] sprintf("    %6d  %s\n", lineno, sym)
		}
	}
	END {
		n = 0
		for (f in count) files[++n] = f
		# sort by file name
		for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (files[j] < files[i]) { t = files[i]; files[i] = files[j]; files[j] = t }
		for (i = 1; i <= n; i++) {
			printf "%5d  %s\n", count[files[i]], files[i]
			if (verbose) printf "%s", items[files[i]]
		}
		printf "%5d  total in %d files\n", total, n
	}'
