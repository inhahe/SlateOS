#!/bin/bash
# Does `date -r` quote the filename in its error, and if so, when?
#
# date.rs uses quote_os (ALWAYS curly-quotes). The harness shows GNU printing
# /nosuch bare. But "never quotes" and "quotes only when the name needs it" both
# explain that one case, and they differ for a name with a space -- which is
# the case that decides between dropping the quoting and switching to quotef_os.
set -u
DATE=/usr/bin/date

run() {
    printf '%-34s %s\n' "$1" "$(LC_ALL=C.UTF-8 "$DATE" -r "$2" 2>&1 | head -1)"
}

echo "=== GNU $($DATE --version | head -1) ==="

# Control: an existing file must NOT produce an error at all, so a run where
# every line looked like an error would be visibly wrong.
touch /tmp/probe_exists.$$
printf '%-34s %s\n' 'CONTROL existing file' \
    "$(LC_ALL=C.UTF-8 "$DATE" -r /tmp/probe_exists.$$ +%Y 2>&1 | head -1)"
rm -f /tmp/probe_exists.$$

run 'plain missing'        '/nosuch'
run 'with a space'         '/no such'
run 'with a quote'         "/no'such"
run 'with a double quote'  '/no"such'
run 'with a backslash'     '/no\such'
run 'with a newline'       "$(printf '/no\nsuch')"
run 'non-ascii'            '/nosüch'
run 'empty'                ''

echo "=== done ==="
