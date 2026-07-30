# The `:h`/`:t`/`:r`/`:e` modifiers read like pathname operations, but readline
# implements each as a single `strrchr` over the *whole* selected text. That
# makes them differ from any sensible basename/extension split in three ways,
# all pinned below: the text comes back unchanged when the separator is missing,
# a leading separator gets no special treatment, and a dot inside a directory
# name counts as the extension. A `:` also *commits* to a modifier — an unknown
# letter after it abandons the line rather than being left as literal text.
#
# Every use below re-issues its own setup line first, so `!!` always names the
# line directly above it no matter what the previous expansion recorded.
set -o history
set -H

# --- the separator is missing: unchanged, not emptied ----------------------
echo abc
echo !!:h
echo abc
echo !!:t
echo abc
echo !!:r
echo abc
echo !!:e

# --- a leading separator is not preserved ----------------------------------
# `:h` of `/abc` is empty (not `/`), and `:t` is the rest.
echo /abc
echo !!:1:h
echo /abc
echo !!:1:t

# --- the dot search ignores `/` entirely -----------------------------------
echo a.b/c
echo !!:1:r
echo a.b/c
echo !!:1:e
# …and a leading dot separates too, so `:r` of `.abc` is empty.
echo .abc
echo !!:1:r
echo .abc
echo !!:1:e

# --- a character that is not a `:` merely ends the modifier run ------------
# `!!:hz` is the `:h` of the event with a literal `z` stuck on the end, and the
# same is true after `:p`, which is why the `z` lands inside what is printed.
echo abc
echo !!:hz
echo abc
echo !!:pz
echo "rc=$?"

# --- but a `:` must be followed by a modifier bash knows -------------------
# Each of these abandons the line without running it and without disturbing `$?`,
# so every `rc=` reports the status of the last command that did run.
echo before-1
echo !!:z
echo "rc=$?"
echo !!:1:z
echo "rc=$?"
echo !!:Z
echo "rc=$?"
# The `g`/`a` prefix is consumed first, so the character *after* it is named.
echo !!:gz
echo "rc=$?"
# Nothing after the `:` at all — bash names an empty character.
echo !!:
echo "rc=$?"
echo !!:g
echo "rc=$?"
echo !!:h:
echo "rc=$?"
# A modifier that already ran is no protection: the next `:` is still checked.
echo before-2
echo !!:s^before^after^:z
echo "rc=$?"

# --- `:s` with no delimiter at all is a no-op, not an error ----------------
echo tail-marker
echo !!:s
echo done
