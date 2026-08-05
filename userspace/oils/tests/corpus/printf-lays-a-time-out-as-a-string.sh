# `%(FORMAT)T` renders the time and then lays the result out as if it were a
# `%s`: the field width pads it with spaces (never zeros) and a *precision
# truncates it*, so `%.2(%Y)T` is `19` and `%.0(%Y)T` is nothing at all.
#
# Its argument is seconds since the epoch, and only two values are sentinels
# rather than times: `-1` is now and `-2` is the shell's start. Every other
# negative is an ordinary instant before the epoch — `-86400` is 1969-12-31 —
# which is easy to get wrong by treating the sign itself as the sentinel.
#
# `%U` and `%W` are the plain week counts, and they are neither the ISO `%V` nor
# each other: week 1 begins at the year's first Sunday for `%U` and its first
# Monday for `%W`, everything before it being week `00`. 2006-01-01 is a Sunday,
# so it is `%U` 01 and `%W` 00 in the same breath.
#
# TZ is fixed here because the fields being compared are dates. `%Z` is left out
# of it: this host's bash names the UTC zone `GMT` where others say `UTC`, and
# that is the zone database talking rather than the shell.

export TZ=UTC
e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }
p() { { eval "$1"; echo "|rc=$?"; } 2>&1 | e; }

echo "== 1. only -1 and -2 are sentinels; the rest are times"
for t in -3 -60 -86400 -100000 -2208988800 -1000000000 0 1 -0 1234567890; do
  printf '  -- [%s]\n' "$t"
  p 'printf "%(%Y-%m-%d %H:%M:%S)T\n" "$t"'
done
echo "--- and the two that are"
printf '%(%Y)T\n' -1 | grep -qE '^(19|20)[0-9][0-9]$' && echo "  -1 is now-ish"
printf '%(%Y)T\n' -2 | grep -qE '^(19|20)[0-9][0-9]$' && echo "  -2 is now-ish"

echo "== 2. precision truncates, width pads with spaces"
p 'printf "[%.2(%Y-%m-%d)T]\n" 0'
p 'printf "[%.0(%Y)T]\n" 0'
p 'printf "[%.20(%Y)T]\n" 0'
p 'printf "[%12.2(%Y)T][%-12.2(%Y)T]\n" 0 0'
p 'printf "[%012.6(%Y-%m-%d)T]\n" 0'
p 'printf "[%012(%Y)T][%-12(%Y)T]\n" 0 0'
p 'printf "[%.4(%Y-%m-%d)T][%.5(%Y-%m-%d)T]\n" 0 0'

echo "== 3. the week counts, against each other and against ISO"
for t in 0 1234567890 1104537600 1136073600 1136160000 978307200 1609459200; do
  printf '  -- [%s]\n' "$t"
  p 'printf "%(%Y-%m-%d %a)T -> U=%(%U)T W=%(%W)T V=%(%V)T G=%(%G)T j=%(%j)T\n" "$t" "$t" "$t" "$t" "$t" "$t"'
done

echo "== 4. a whole year of Januaries, where the three disagree most"
for y in 2000 2001 2002 2003 2004 2005 2006 2007 2008 2009 2010 2011; do
  t=$(( (y - 1970) * 31536000 ))
  printf '%(%Y-%m-%d %a)T U=%(%U)T W=%(%W)T V=%(%V)T\n' "$t" "$t" "$t" "$t"
done

echo "== 5. the reused format still consumes one argument each"
p 'printf "%(%Y)T-%(%m)T|" 0 0 86400000 86400000; echo'
