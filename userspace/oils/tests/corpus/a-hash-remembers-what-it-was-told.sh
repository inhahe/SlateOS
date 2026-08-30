# `hash` is the table of remembered command locations, and it can be written to
# directly: `hash -p PATH NAME` files a location without looking for it, so
# every row below names a path that does not exist and never has to be found.
#
# The table prints three ways. Bare, it is a two-column report with a `hits`
# header — or the line `hash: hash table empty` when there is nothing to show,
# which goes to *stdout* at status 0, not to stderr. `-l` prints it as
# re-inputtable `builtin hash -p` commands instead. `-t NAME` prints just one
# location, and with more than one name it prefixes each with the name it
# belongs to. `-r` forgets everything; `-d NAME` forgets one.
#
# A name that is not in the table is looked up: `hash -t` on it is
# `NAME: not found` at status 1, and so is a bare `hash NAME` that no `$PATH`
# entry answers. `hash -d` on such a name is *not* an error — there was
# nothing to remove and now there is still nothing. `-` is not an option, so
# it is a name, and no `$PATH` entry is called `-`.
#
# Deliberately absent:
#
#   * every location that was actually found. The two shells resolve the same
#     command to differently-spelled host paths (`/usr/bin/cat` vs
#     `C:\…\cat.exe`) on the dev host, so a real lookup's *path* is never
#     printed here — only whether it succeeded.
#
# Every probe runs in a subshell so a table entry cannot reach the next one.
# Stderr is collected and replayed at the end so it can be compared in a fixed
# place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err

echo "=== an empty table"
( hash; echo "  bare     rc=$?" )
( hash -r; echo "  -r       rc=$?" )
( hash -l; echo "  -l       rc=$?" )
( hash -t nosuchprogram; echo "  -t       rc=$?" )
( hash -d nosuchprogram; echo "  -d       rc=$?" )
( hash nosuchprogram; echo "  name     rc=$?" )

echo "=== -p files a location without looking for it"
( hash -p /bin/nosuch xx; hash; echo "  bare     rc=$?" )
( hash -p /bin/nosuch xx; hash -t xx; echo "  -t       rc=$?" )
( hash -p /bin/nosuch xx; hash -l; echo "  -l       rc=$?" )
( hash -p /bin/nosuch xx yy; hash; echo "  two      rc=$?" )
( hash -p /bin/nosuch xx; hash -p /bin/other yy; hash; echo "  again    rc=$?" )
( hash -p /bin/nosuch xx; hash -p /bin/other xx; hash; echo "  rebind   rc=$?" )
( hash -p /bin/nosuch; hash; echo "  no name  rc=$?" )

echo "=== forgetting"
( hash -p /bin/nosuch xx; hash -r; hash; echo "  -r       rc=$?" )
( hash -p /bin/nosuch xx; hash -d xx; hash; echo "  -d       rc=$?" )
( hash -p /bin/nosuch xx yy; hash -d xx; hash; echo "  -d one   rc=$?" )
( hash -p /bin/nosuch xx; hash -d xx; hash -t xx; echo "  gone     rc=$?" )

echo "=== -t names more than one location"
( hash -p /bin/nosuch xx yy; hash -t xx yy; echo "  two      rc=$?" )
( hash -p /bin/nosuch xx; hash -t xx nosuchprogram; echo "  miss     rc=$?" )
( hash -p /bin/nosuch xx; hash -t nosuchprogram xx; echo "  first    rc=$?" )

echo "=== a real lookup, without saying where it landed"
( hash cat; echo "  add      rc=$?" )
( hash cat; hash -t cat >/dev/null; echo "  -t       rc=$?" )
( hash cat; hash -d cat; hash; echo "  -d       rc=$?" )

echo "=== option errors"
( hash -z; echo "  -z       rc=$?" )
( hash -rz; echo "  -rz      rc=$?" )
( hash --; echo "  --       rc=$?" )
( hash -; echo "  -        rc=$?" )
( hash -r xx; echo "  -r name  rc=$?" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
