# `type` and `command -v`/`-V` answer the same question — what would this word
# run? — and share one precedence: **alias > keyword > function > builtin >
# `$PATH` file**. They differ in what they print. `type` writes a sentence
# ("f is a function", and for a function the reconstructed body after it);
# `command -V` writes the same sentence; `command -v` writes the *terse* form,
# which is the bare name for everything but an alias (a re-inputtable
# `alias name='value'`) and a file (its path).
#
# A name can mean several things at once, and `-a` is what asks for all of
# them. `-t` is not a competing mode but a change of wording — it names the
# *kind* rather than describing it — so the two compose: `type -at` prints one
# kind word per meaning, in the same order, and one `file` per `$PATH` match
# rather than one for all of them. Both letters bundle, in either order.
#
# The three ways of not finding a name differ. A bare `type` and `type -a` say
# `NAME: not found` at status 1; `-t`, `-p` and `-P` are silent at status 1;
# and either way the names after the missing one are still answered. `command`
# is the same shape with the halves swapped: `-V` announces a miss, `-v` is
# silent, and the *status* is whether any name at all was described — so one
# hit rescues a list that also missed.
#
# `command`'s options are letters in a bundle, not whole words, and `-v` and
# `-V` are one setting rather than two flags: whichever comes last wins, so
# `-vV` describes verbosely and `-Vv` tersely. `-p` only rides along.
#
# Deliberately absent:
#
#   * every path, and so every probe that would resolve to a `$PATH` file.
#     The two shells are handed *different* `$PATH`s on the dev host — bash
#     under MSYS gets `:`-separated POSIX names, osh the Windows `;`-separated
#     ones — so they find a different number of `cat`s in different places,
#     and neither the paths nor their count can be compared. `type -P` appears
#     only with its output discarded, for its status.
#   * `type` with no name at all, which is status 0 and no output.
#
# Every probe runs in a subshell so a definition cannot reach the next one.
# Stderr is collected and replayed at the end so it can be compared in a fixed
# place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err
shopt -s expand_aliases
f() { :; }
alias al='echo hi'

echo "=== the four kinds that are not files"
( type f; echo "  func   rc=$?" )
( type al; echo "  alias  rc=$?" )
( type if; echo "  keyw   rc=$?" )
( type cd; echo "  built  rc=$?" )
( type nosuchname; echo "  miss   rc=$?" )

echo "=== -t names the kind instead of describing it"
( type -t f; echo "  func   rc=$?" )
( type -t al; echo "  alias  rc=$?" )
( type -t if; echo "  keyw   rc=$?" )
( type -t cd; echo "  built  rc=$?" )
( type -t nosuchname; echo "  miss   rc=$?" )

echo "=== a name can mean several things at once"
( cd() { :; }; alias cd='echo x'; type cd; echo "  plain  rc=$?" )
( cd() { :; }; alias cd='echo x'; type -a cd; echo "  -a     rc=$?" )
( cd() { :; }; alias cd='echo x'; type -t cd; echo "  -t     rc=$?" )
( cd() { :; }; alias cd='echo x'; type -at cd; echo "  -at    rc=$?" )
( cd() { :; }; alias cd='echo x'; type -ta cd; echo "  -ta    rc=$?" )
( cd() { :; }; alias cd='echo x'; type -t -a cd; echo "  -t -a  rc=$?" )
( cd() { :; }; alias cd='echo x'; type -f cd; echo "  -f     rc=$?" )
( cd() { :; }; alias cd='echo x'; type -af cd; echo "  -af    rc=$?" )
( alias if='echo x'; type -at if; echo "  keyw   rc=$?" )
( type -at f; echo "  one    rc=$?" )
( type -at f al; echo "  two    rc=$?" )
( type -at f nosuchname al; echo "  gap    rc=$?" )

echo "=== -f skips the function, -p and -P want a file"
( type -f f; echo "  -f f   rc=$?" )
( type -f cd; echo "  -f cd  rc=$?" )
( type -p f; echo "  -p f   rc=$?" )
( type -p cd; echo "  -p cd  rc=$?" )
( type -p nosuchname; echo "  -p mis rc=$?" )
( type -P f >/dev/null; echo "  -P f   rc=$?" )
( type -P nosuchname; echo "  -P mis rc=$?" )

echo "=== a miss does not stop the names after it"
( type f nosuchname al; echo "  plain  rc=$?" )
( type -a nosuchname f; echo "  -a     rc=$?" )
( type -t f nosuchname cd; echo "  -t     rc=$?" )

echo "=== type's option errors"
( type -z f; echo "  -z     rc=$?" )
( type; echo "  none   rc=$?" )
( type --; echo "  --     rc=$?" )
( type -- f; echo "  -- f   rc=$?" )
( type -; echo "  -      rc=$?" )

echo "=== command -v is terse where -V is a sentence"
( command -v f; echo "  -v f   rc=$?" )
( command -v al; echo "  -v al  rc=$?" )
( command -v if; echo "  -v if  rc=$?" )
( command -v cd; echo "  -v cd  rc=$?" )
( command -V f; echo "  -V f   rc=$?" )
( command -V al; echo "  -V al  rc=$?" )
( command -V if; echo "  -V if  rc=$?" )
( command -V cd; echo "  -V cd  rc=$?" )

echo "=== and only -V announces a miss"
( command -v nosuchname; echo "  -v     rc=$?" )
( command -V nosuchname; echo "  -V     rc=$?" )
( command -v nosuchname f; echo "  -v mix rc=$?" )
( command -V nosuchname f; echo "  -V mix rc=$?" )
( command -V f nosuchname; echo "  -V rev rc=$?" )
( command -v nosuchname nosuchother; echo "  -v two rc=$?" )
( command -V nosuchname nosuchother; echo "  -V two rc=$?" )
( command -v f cd; echo "  both   rc=$?" )

echo "=== command's letters bundle, and the last of -v/-V wins"
( command -vV f; echo "  -vV    rc=$?" )
( command -Vv f; echo "  -Vv    rc=$?" )
( command -pv f; echo "  -pv    rc=$?" )
( command -vp f; echo "  -vp    rc=$?" )
( command -v -- f; echo "  -v --  rc=$?" )
( command -v; echo "  bare   rc=$?" )
( command -V; echo "  bareV  rc=$?" )
( command -p; echo "  -p     rc=$?" )
( command -z f; echo "  -z     rc=$?" )
( command -pz f; echo "  -pz    rc=$?" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
