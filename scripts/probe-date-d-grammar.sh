#!/bin/bash
# Measure GNU date's -d language, as epoch seconds.
#
# `+%s` rather than the default rendering on purpose: it collapses zone and
# format out of the answer and leaves only the INSTANT, which is the thing the
# parser decides. Two forms that print differently but mean the same moment
# should be visibly the same here.
#
# TZ is pinned. -d resolves bare dates in LOCAL time, so an unpinned zone makes
# every absolute case below a property of the host.
set -u
export TZ=UTC
DATE=/usr/bin/date

d() {
    local out rc
    out=$("$DATE" -d "$1" +%s 2>&1)
    rc=$?
    printf '%-34s rc=%-3s %s\n' "[$1]" "$rc" "$out"
}

echo "=== GNU $($DATE --version | head -1), TZ=$TZ ==="

# Control: a form known to work, and one known to fail. If everything came
# back rc=0 or everything rc=1, the probe would be measuring nothing.
d '@0'
d 'not a date at all'

echo "--- absolute ---"
d '2021-03-04 05:06:07'
d '2021-03-04T05:06:07'
d '2021-03-04'
d '2021-03-04 05:06:07 UTC'
d '2021-03-04 05:06:07 +0200'
d '2021-03-04 05:06:07 -0500'
d '1970-01-01 00:00:00 UTC'
d 'Mar 4 2021'
d '4 March 2021'
d 'March 4, 2021'
d '4 Mar 2021 05:06:07'
d '2021/03/04'
d '03/04/2021'

echo "--- time only (date defaults to TODAY) ---"
d '05:06:07'
d '05:06'
d '5:06:07 PM'
d '05:06:07.5'

echo "--- keywords ---"
d 'epoch'
d 'now'
d 'today'
d 'tomorrow'
d 'yesterday'

echo "--- relative against a fixed base ---"
d '@0 + 1 day'
d '@0 +1 day'
d '@0 1 day'
d '@0 - 1 day'
d '@0 + 2 weeks'
d '@0 + 1 hour'
d '@0 + 1 minute'
d '@0 + 1 second'
d '@0 + 1 month'
d '@0 + 1 year'
d '@0 3 days ago'
d '@0 next day'
d '@0 last day'

echo "--- malformed ---"
d ''
d '   '
d '@'
d '@@0'
d '2021-13-04'
d '2021-03-32'
d '25:00:00'

echo "=== done ==="
