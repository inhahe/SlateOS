#!/bin/bash
# When does GNU diff QUOTE a filename in its own output, and in what style?
#
# Found by adding non-UTF-8 fixture names to scripts/diff-diff.sh: GNU prints
#   --- "da/odd\351name.txt"
# where ours prints the raw bytes. Double quotes and an octal escape is the C
# style, not the shell style `date -r` turned out to use -- so the two cannot
# be assumed to share a helper, and this measures which is which.
set -u
DIFF=/usr/bin/diff
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
cd "$tmp" || exit 1

mk() { printf 'a\n' > "$1"; printf 'b\n' > "$2"; }

show() {
    local label="$1" f1="$2" f2="$3"
    printf '%-22s %s\n' "$label" "$("$DIFF" -u "$f1" "$f2" 2>&1 | head -2 | tr '\n' '|')"
}

echo "=== GNU $($DIFF --version | head -1) ==="

# Control: an ordinary name must NOT be quoted. Without this, a helper that
# quotes everything would look correct.
mk plain1.txt plain2.txt
show 'CONTROL plain'    plain1.txt plain2.txt

mk "$(printf 'hi\351there')" "$(printf 'ho\351there')"
show 'non-utf8 byte'    "$(printf 'hi\351there')" "$(printf 'ho\351there')"

mk 'with space1' 'with space2'
show 'space'            'with space1' 'with space2'

mk 'tab	one' 'tab	two'
show 'tab'              'tab	one' 'tab	two'

mk 'quo"te1' 'quo"te2'
show 'double quote'     'quo"te1' 'quo"te2'

mk "apo'st1" "apo'st2"
show 'single quote'     "apo'st1" "apo'st2"

mk 'back\slash1' 'back\slash2'
show 'backslash'        'back\slash1' 'back\slash2'

mk 'utf8é1' 'utf8é2'
show 'valid utf-8'      'utf8é1' 'utf8é2'

echo
echo "=== the -r label and Only-in lines ==="
mkdir -p ra rb
printf 'a\n' > "ra/$(printf 'odd\351')"
printf 'b\n' > "rb/$(printf 'odd\351')"
printf 'a\n' > "ra/$(printf 'lone\351')"
"$DIFF" -r ra rb 2>&1 | head -6
