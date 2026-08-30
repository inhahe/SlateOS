# A nameref walk gives up two ways — it closes on a name it has already stood
# on, or it runs past `NAMEREF_MAX` links — and `find_variable_nameref`
# (variables.c:2074) answers NULL for both. Which of the two a chain does is
# not a property of the chain alone: every link after the first is an ordinary
# innermost-first lookup, so a **local shadow** can re-enter the chain and
# close it.
#
#   * `g→z, z→g` with a frame-local `g` in the way is no cycle to a live walk:
#     it reads `g(global)→z→g(local)→…` and the local answers.
#   * `g→z, z→y, y→z` never names `g` again, so nothing a frame holds can
#     close it, and the walk spins to the limit.
#
# Where the *global-only* walk gives up, a compound `declare -g` parts into two
# variables. Step 1 of the decomposition is `declare -g g` — bare, since
# `fix_assignment_words` (execute_cmd.c:4180-4249) sets `W_ASSIGNARRAY` only
# from a preceding `-a` **option word** and never from the literal's own shape —
# so it is `bind_global_variable`, whose `bind_variable_internal`
# (variables.c:3081) opens on the global table's own nameref entry:
#
#     entry = find_global_variable (entry->name);
#     if (entry == 0) entry = find_variable_last_nameref (name, 0);
#
# — the **live** chain's last reference cell. Step 2's `find_global_variable`
# then repeats the global-only walk, which gives up again, and it binds the name
# as written. The two land apart.

echo '=== the limit is reached, so step 1 goes elsewhere and the literal stays'
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local -n g=w; declare -g g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z y w )

echo '=== a chain a local closes answers, so one name serves both'
( declare -n g=z; declare -n z=g
  f() { local -n g=w; declare -g g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z w )

echo '=== the same two chains, and a scalar operand with no step 1 at all'
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z y w )
( declare -n g=z; declare -n z=g
  f() { local -n g=w; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z w )

echo '=== a live binding that is no reference is emptied where it stands'
# `find_variable_last_nameref` answers with the last variable it *stood on*, so
# a frame-local that is no reference is itself the answer — and
# `bind_variable_internal` falls through to `assign_value:` and writes it in
# place, local or not. Step 1's value is NULL, so what it writes there is
# nothing, and the literal after it goes on to bind the global. A scalar
# operand has no step 1 to separate from: the builtin *is* the one command, and
# its value stays on the local, which dies with the frame.
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local g=5; declare -g g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z y )
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local g=5; declare -g g=1; declare -p g; }; f; echo AFTER; declare -p g z y )

echo '=== with nothing live to fall back on there is nothing to leave behind'
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { declare -g g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z y )

echo '=== `readonly` asks the live chain for the literal too, so it does not part'
# `nameref_transform_name` (variables.c:2333) reads `ASS_CHKLOCAL` — the word
# flag `readonly` and `export` carry standingly (execute_cmd.c:4221), which no
# letter of `declare` sets — and where it is set the literal walks the live
# chain, arriving exactly where step 1 did.
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local -n g=w; readonly g=(1 2); }; f; echo AFTER; declare -p g z y w )
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local -n g=w; export g=(1 2); }; f; echo AFTER; declare -p g z y w )

echo '=== `-G` is the builtin half alone, so its literal parts from step 1 still'
# The letter is read by `declare_internal` and never by `fix_assignment_words`,
# so the operand carries no `W_CHKLOCAL` and the literal takes the global-only
# road — the parting above, and only the builtin proper follows step 1 out.
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local -n g=w; declare -gG g=(1 2); }; f; echo AFTER; declare -p g z y w )

echo '=== a reference into the frame is a local chklocal accepts, and it dies there'
( declare -n g=z; declare -n z=y; declare -n y=z
  f() { local w; local -n g=w; readonly g=(1 2); }; f; echo AFTER; declare -p g z y w )

echo '=== a longer overrun reads no differently'
( declare -n g=z; declare -n z=y; declare -n y=x; declare -n x=y
  f() { local -n g=w; declare -g g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z y x w )
