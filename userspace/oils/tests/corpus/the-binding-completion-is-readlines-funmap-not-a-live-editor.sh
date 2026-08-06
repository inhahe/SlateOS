# `compgen -A binding` answers with readline's *funmap* — every function name
# readline knows how to run — and it answers in a script, where there is no line
# editor at all. The two facts sit oddly together only until one notices what
# the list is made of: `rl_funmap_names()` reads a table that readline compiles
# in, so the names exist as soon as the library is linked, whether or not a
# terminal ever asks one of them to do anything.
#
# It is the same list `bind -l` prints, in the same order — sorted, because
# readline sorts the funmap itself — which is the useful way to say what the
# action is. `bind -l` warns first that line editing is not enabled and then
# prints all 174 names regardless; `compgen -A binding` skips the warning and
# prints the same names. A shell that cannot *perform* `yank-last-arg` still
# knows the name of it.
#
# Deliberately absent: `bind -q`, `bind -p` and the rest of the query surface,
# which have cases of their own; this one is only about the completion action
# and its relationship to `bind -l`.

echo "=== the two lists are the same list"
bind -l > bl.txt 2>bl.err
echo "  bind -l   rc=$? lines=$(wc -l < bl.txt)"
echo "  warned:   [$(cat bl.err)]"
compgen -A binding > cb.txt 2>cb.err
echo "  compgen   rc=$? lines=$(wc -l < cb.txt) err=[$(cat cb.err)]"
cmp -s bl.txt cb.txt && echo "  identical" || echo "  DIFFER"

echo "=== …and it is sorted, which is how the funmap is kept"
diff <(sort bl.txt) bl.txt > /dev/null && echo "  sorted" || echo "  unsorted"
head -3 cb.txt
tail -2 cb.txt

echo "=== a prefix narrows it like any other action"
compgen -A binding vi-y
echo "  rc=$?"
compgen -A binding yank
echo "  rc=$?"
compgen -A binding zzz
echo "  none rc=$?"
# The action is spelled only in full — there is no single letter for it, so a
# `-A` is the only way in, and an unknown action is a usage error.
compgen -A bindings x 2> ba.err
echo "  bad-action rc=$?"
cat ba.err

echo "=== it joins the other actions in the action table's own order"
# `binding` sorts before `builtin`, so its names come first however the two are
# written on the command line.
compgen -A binding -A builtin br
compgen -A builtin -A binding br
echo "  rc=$?"

echo "=== and a -W wordlist still comes after every action"
compgen -W 'brzzz' -A binding br
echo "  rc=$?"
