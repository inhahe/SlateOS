# `M>&N` and `M<&N` are a `dup2`: descriptor M becomes a second name for
# whatever N already is. Three things follow, and a shell that models redirects
# as "where does stdout go" gets all three wrong once M is neither 1 nor 2:
#
#   * the source has to exist. `7<&9` with fd 9 closed is as dead as `<&9` is,
#     and takes the command down with it — a redirector other than the
#     operator's own is not an excuse to skip the check;
#   * duplicating a descriptor onto *itself* is not a dup at all. bash skips the
#     call, so `7>&7` succeeds even with fd 7 closed and nothing to duplicate:
#     it is the one form that needs no source;
#   * and the dup rebinds fd 7, not stdout. Only `exec` and a scoped redirect on
#     a compound command can be *seen* to do it, since nothing the shell writes
#     itself goes through fd 7 — but a command carrying one must not have its
#     ordinary output diverted into it, and the binding must not outlive a scope
#     that owns it.
#
# The read side of the last point is the cursor: two names for one input
# descriptor share a position, so reading through either one advances both.
#
# Stderr is collected and replayed at the end so it can be compared in a fixed
# place; nothing here prints a pid, so it is replayed unfiltered.
exec 3>&2 2>err
printf 'one\ntwo\nthree\n' > in
exec 5>f5
exec 6>f6
exec 9<in

echo "=== a missing source is fatal whatever the redirector"
{ echo scoped; } 7>&8;  echo "  7>&8 rc=$?"
{ echo scoped; } 7<&8;  echo "  7<&8 rc=$?"
read -r l 4<&8;         echo "  4<&8 rc=$?"
echo hi 4>&8;           echo "  4>&8 rc=$?"

echo "=== but a descriptor is always its own source"
{ echo "  self-out"; } 7>&7; echo "  7>&7 rc=$?"
{ echo "  self-in";  } 7<&7; echo "  7<&7 rc=$?"

echo "=== an alias rebinds its own descriptor, not stdout"
{ echo A >&5; echo B >&7; echo C; } 7>&6; echo "  brace rc=$?"
echo "=== and does not outlive the command that made it"
echo D >&7; echo "  D rc=$?"

echo "=== exec makes the same alias persist"
exec 7>&6; echo "  exec rc=$?"
echo E >&7; echo "  E rc=$?"
echo F;     echo "  F rc=$?"

echo "=== two names for one input share a cursor"
{ read -r l <&8; echo "  first=[$l]"; } 8<&9
read -r l <&9;   echo "  next=[$l]"

exec 5>&- 6>&- 7>&- 9<&-
echo "f5=[$(cat f5)]"
echo "f6=[$(cat f6)]"

exec 2>&3 3>&-
echo "=== what went to stderr"
cat err
