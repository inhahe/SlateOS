# An assignment-only command answers with the status of the last command
# substitution in it — `z=$(exit 3)` is 3 — and resets `$?` to 0 when there was
# none. A write refused because the shell maintains the name is one more thing
# that sets that status, to 1, as the words are worked through in order. So
# which of the two answers depends on which came *last*:
#
#   * `FUNCNAME=$(exit 3)` is 1 — the substitution ran while the value was being
#     expanded, and the refusal came after it;
#   * `FUNCNAME=1 z=$(exit 3)` is 3 — here the substitution came after;
#   * `FUNCNAME=5` is 0, because a command with no substitution at all is reset
#     to 0 last of all, and an arithmetic expansion is not a substitution.
#
# It is the whole command that is judged, not the one word: the refusal may be
# in any of them, and so may the substitution. With a command word in front
# there is no assignment-only command left to judge, so the command answers for
# itself.

echo '=== the refusal replaces a substitution status, and only that'
( FUNCNAME=5; echo "plain: $?" ) 2>&1
( FUNCNAME=$((1+1)); echo "arith: $?" ) 2>&1
( FUNCNAME=$(exit 0); echo "sub0: $?" ) 2>&1
( FUNCNAME=$(exit 3); echo "sub3: $?" ) 2>&1
( FUNCNAME=`exit 3`; echo "backtick: $?" ) 2>&1
( FUNCNAME+=$(exit 3); echo "append: $?" ) 2>&1
( GROUPS=$(exit 3); echo "groups: $?" ) 2>&1
( BASH_SOURCE=$(exit 3); echo "source: $?" ) 2>&1

echo '=== …and it is the command that is judged, not the word'
( z=$(exit 3) FUNCNAME=1; echo "before: $?" ) 2>&1
( FUNCNAME=$(exit 3) z=1; echo "after: $?" ) 2>&1
( FUNCNAME=1 z=$(exit 0); echo "other: $?" ) 2>&1
( z=$(exit 3) q=1; echo "none: $?" ) 2>&1

echo '=== with a command word the command answers for itself'
( FUNCNAME=$(exit 3) true; echo "true: $?" ) 2>&1
( FUNCNAME=$(exit 3) false; echo "false: $?" ) 2>&1
( FUNCNAME=$(exit 3) : ; echo "colon: $?" ) 2>&1

echo '=== a refusal inside a substitution stays that command'"'"'s business'
( z=$(FUNCNAME=$(exit 0); echo hi); echo "nested: $? [$z]" ) 2>&1
( z=$(FUNCNAME=5); echo "nested plain: $? [$z]" ) 2>&1

echo '=== an ordinary name is untouched, and so is a dynamic special'
( q=$(exit 3); echo "ordinary: $?" ) 2>&1
( SECONDS=$(exit 3); echo "seconds: $?" ) 2>&1
( LINENO=$(exit 3); echo "lineno: $?" ) 2>&1

echo '=== a declaration builtin answers by its own rules'
( declare FUNCNAME=$(exit 3); echo "declare: $?" ) 2>&1
( export FUNCNAME=$(exit 3); echo "export: $?" ) 2>&1
( readonly FUNCNAME=$(exit 3); echo "readonly: $?" ) 2>&1

echo '=== errexit sees an ordinary failure'
( set -e; FUNCNAME=$(exit 0); echo "after errexit" ) 2>&1
( trap 'echo ERR' ERR; FUNCNAME=$(exit 0); echo "after trap: $?" ) 2>&1

echo '=== a substitution after the refusal takes the status back'
( FUNCNAME=1 z=$(exit 3); echo "after: $?" ) 2>&1
( z=$(exit 0) FUNCNAME=1; echo "before: $?" ) 2>&1
( q=$(exit 3) FUNCNAME=1 w=$(exit 0); echo "last wins: $?" ) 2>&1
( q=$(exit 0) FUNCNAME=1 w=$(exit 3); echo "last wins too: $?" ) 2>&1
( FUNCNAME=1 GROUPS=$(exit 3); echo "two refusals: $?" ) 2>&1
( FUNCNAME=$(exit 3) GROUPS=1; echo "two refusals too: $?" ) 2>&1

echo '=== a compound value is refused outright, and discards the rest'
( FUNCNAME=(1 2); echo "unreached: $?" ) 2>&1
( FUNCNAME=(1 2) ) ; echo "compound: $?"
( GROUPS=($(exit 3)); echo "unreached: $?" ) 2>&1

echo '=== inside a function the name is the same name'
( f() { FUNCNAME=$(exit 0); echo "fn: $?"; }; f ) 2>&1
( f() { FUNCNAME=1 z=$(exit 3); echo "fn after: $?"; }; f ) 2>&1

echo still here
