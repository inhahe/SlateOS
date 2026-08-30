# `case` is the one compound command that expands words of its own — the
# subject once, then each pattern in turn until one matches — and bash never
# returns from a failure in one of them. `execute_case_command` calls
# `expand_word_unsplit` on the subject and `expand_word_leave_quoted` on each
# pattern, and a fatal expansion error inside either does
# `jump_to_top_level` straight out of the case: no arm is tested, no arm runs,
# and whatever else was in the same parse unit is gone with it.
#
# The two depths bash raises those from unwind differently, and `case` has to
# tell them apart the same way a simple command does:
#
#   * a *discard* — a bad array subscript in a substitution, or a `$( … )`
#     whose re-print will not parse back — drops one parse unit and leaves
#     `$?` at 1, and the shell reads on;
#   * a nounset reference under `set -u`, a `${x:?word}`, or a bad
#     substitution ends the **shell**, with the status it carried.
#
# Neither is the same as "no arm matched", which is the silent 0 a `case` with
# nothing to match yields — so a `case` that aborted must not be allowed to
# fall out of the bottom and report that.

echo "=== a failed subject ends the case: no arm is tested"
( case "y${nope?bad}" in *) echo arm ;; esac; echo "not reached" )
echo "rc=$?"

echo "=== and a failed pattern ends it just as dead"
( case x in "y${nope?bad}") echo arm ;; *) echo star ;; esac; echo "not reached" )
echo "rc=$?"

echo "=== the patterns after the failing one are not looked at either"
( case x in "y${nope?bad}") ;; "z${alsonope?worse}") ;; esac )
echo "rc=$?"

echo "=== an aborted case is not a case that merely did not match"
( case q in x) ;; esac )
echo "no match rc=$?"
( case "y${nope?bad}" in x) ;; esac )
echo "aborted rc=$?"

echo "=== set -u reaches the same two places"
( set -u; case "y$nope" in *) echo arm ;; esac )
echo "subject rc=$?"
( set -u; case x in "y$nope") echo arm ;; esac )
echo "pattern rc=$?"

echo "=== a discard drops the parse unit instead, and the shell reads on"
case x in "y$(
!
)z") echo arm ;; esac; echo "not reached"
echo "after rc=$?"

echo "=== and written as the subject, where the line is the case's own"
case "y$(
!
)z" in *) echo arm ;; esac; echo "not reached"
echo "after rc=$?"

echo "=== a subject that expands cleanly still matches normally"
n=q
case "y$n" in yq) echo matched ;; *) echo star ;; esac
echo "rc=$?"
