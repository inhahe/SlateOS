# A simple command whose words all vanish still ran something: the
# substitutions written in it. bash keeps the status of the last one.
#
#     /* Even if there aren't any command names, pretend to do the
#        redirections that are specified. … If the redirections fail, then
#        return failure.  Otherwise, if a command substitution took place while
#        expanding the command or a redirection, return the value of that
#        substitution.  Otherwise, return EXECUTION_SUCCESS. */
#     r = do_redirections (…);
#     if (r != 0)              return (EXECUTION_FAILURE);
#     else if (last_command_subst_pid != NO_PID) return (last_command_exit_value);
#     else                     return (EXECUTION_SUCCESS);
#
# — `execute_null_command` (execute_cmd.c:4143-4160), reached from
# `execute_simple_command` (4515-4531) when `expand_words` came back empty:
# "It is possible for WORDS not to have anything left in it. Perhaps all the
# words consisted of `$foo', and there was no variable `$foo'."
#
# The rule is usually met as the *pure assignment* one — `x=$(false)` leaves 1
# — but the assignment is not what carries it. `last_command_subst_pid` is
# cleared once per simple command (4416) and set by every `$( … )` the command
# performs, wherever it was written, so a word list that expands away carries
# the status just as an assignment list does.
#
# Order matters where both have one. `expand_word_list_internal`
# (subst.c:12965) splits the assignments off with `separate_out_assignments`,
# expands what is left, and only then runs `do_assignment_statements` — so the
# assignments' substitutions are the later ones and theirs is the status kept.
#
# Two things stop a command from being nameless in the first place: a *quoted*
# empty word is a name (`"$(exit 5)"` is command-not-found, 127), and a word
# that is only partly a substitution keeps whatever else it had.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== 1. the word list's substitution is the status"
e '$(exit 5)'
e '`exit 5`'
e '$(false)'
e '`false`'
e '$(true)'
e '$(exit 0)'
# Several: the last one performed wins.
e '`exit 5``exit 7`'
e '$(exit 5) $(exit 7)'
e '$(exit 5) $(exit 0)'
e '$(exit 7) $(exit 5) $(exit 3)'
# A body that does not parse is a status too.
e '`fi`'
e '$(fi)'
e '$(for)'
# And a word that is only partly a substitution is still a word.
e '`fi`x'
e 'x$(exit 5)'
# A quoted empty word is a name, so the command is not nameless at all.
e '"$(exit 5)"'
e "'' "
# Nothing was performed, so there is nothing to keep.
e '${nope-}'
e '$nope'
e '${nope-}${also-}'

echo "=== 2. an earlier status is not kept"
e '(exit 7); ${nope-}'
e 'false; ${nope-}'
e '(exit 7); $(true)'
e 'false; $(exit 0)'

echo "=== 3. assignments are performed after the words"
e 'x=$(exit 5)'
e 'x=1 $(exit 5)'
e 'x=$(exit 7) $(exit 5)'
e 'a=$(exit 3) b=$(exit 4) $(exit 5)'
e '$(exit 5) x=$(exit 7)'
e 'x=1 y=2 $(exit 5)'
e 'x=1 y=2'
# Being second is visible from inside the value as well: `$(true)` in the word
# list has already run and already set `$?`, so the assignment reads 0, not the
# 9 that was current when the command started.
e '(exit 9); x=$? $(true); echo "x=$x"'
e '(exit 9); x=$? ${nope-}; echo "x=$x"'

echo "=== 4. redirections"
# A redirection that fails outranks the substitution.
e '$(exit 5) < nosuchfile'
e '< $(exit 5)'
e '$(exit 5) > /dev/null'
e '> /dev/null $(exit 5)'
# A substitution performed in the redirection counts as much as one in a word.
e '> $(echo out); echo made=$?'
e 'exec 9>&- ; $(exit 5) 2>/dev/null'
e '${nope-} > "$(echo out)"'
e '${nope-} > "$(echo out; exit 5)"'
# …but only if the redirection then works: `exit 5` cuts the body off before it
# printed anything, so the target is the empty name and the failure outranks.
e '${nope-} > "$(exit 5; echo out)"'

echo "=== 5. what it is not"
# A name makes it an ordinary command again.
e 'echo $(exit 5) >/dev/null'
e ': $(exit 5)'
e 'true $(exit 5)'
# The substitution is performed inside another one, so the outer is the last.
e '$(echo $(exit 5))'
e '$( $(exit 5) )'
# A defaulting assignment performs it; a `+` alternate with the name unset does
# not.
e 'unset u; ${u:=$(exit 5)}'
e 'unset u; ${u-$(exit 5)}'
e 'unset u; ${u+$(exit 5)}'
e 'u=1; ${u+$(exit 5)}'

echo "=== 6. where the status goes next"
e '$(exit 5) | cat; echo "PS=${PIPESTATUS[*]}"'
e 'set -e; $(exit 5); echo not reached'
e 'set -e; ${nope-}; echo reached'
e 'trap "echo ERR=\$?" ERR; $(exit 5)'
e 'f() { $(exit 5); }; f'
e 'f() { $(exit 5); }; f; echo after=$?'
e 'if `fi`; then echo t; else echo f=$?; fi'
e '( `fi` )'
e '{ $(exit 5); }'
e '$(exit 5) &
wait $!'
# `bind_lastarg (NULL)` runs with no last argument to bind.
e 'echo keep; $(true); echo "_=[$_]"'

echo "=== 7. with IFS emptied, and with the field split away"
e 'IFS=; $(exit 5)'
e 'IFS=; x=" "; $x'
e 'x="  "; $x'
e 'x="  "; $x; echo n=$?'

echo done
