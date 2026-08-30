# `enable` turns a builtin off (`-n`) so the name reaches `$PATH` instead, and
# prints which ones are in which state. The listing is a set of re-inputtable
# `enable` commands, one per builtin, in the shell's own sorted order; `-n`
# narrows it to the ones that are *off*, and `-s` narrows it to the POSIX
# *special* builtins — the fifteen POSIX names plus bash's `source`. The two
# narrowings compose, and `-p` asks explicitly for the form the listing already
# has. Given names instead, `-s` is only a filter: a name that is not special
# is silently skipped, and only a name that is no builtin at all is an error.
#
# `-f filename` loads a builtin from a shared object and `-d` unloads one, so
# what they do depends on whether the shell has dynamic loading at all — and
# that is a property of the *build*, not of bash. A bash configured without
# `dlopen` refuses both outright; the glibc bash this corpus runs against has
# it, and so really tries: `-f /nosuch` is `cannot open shared object …` at
# status 1, and `-d echo` is `echo: not dynamically loaded`, also at 1.
#
# osh follows the no-`dlopen` build, because that is the only honest answer it
# has: SlateOS has no dynamic linker, so there is no shared object for osh to
# load on the machine it is the shell for. (That osh "needs no dynamic linker"
# is one of the stated reasons it ships as the shell — design-decisions §305.)
# The probes whose answers hinge on that difference are therefore left out
# rather than waived, and the gap is recorded in known-issues as out of scope.
# See TD-OILS-ENABLE-F-AND-D-FOLLOW-THE-NO-DLOPEN-BUILD.
#
# What *is* probed here is the part both builds share, because it happens in
# the option parser before either letter is acted on: an `-f` with nothing left
# to take is `enable: -f: option requires an argument` *with* a usage line, and
# letters are scanned left to right across words and within bundles, so `-nf`
# is refused for `f` and `-sz` for `z`, each on the first letter that objects.
#
# The *full* listing is here too, and it is the count that matters: both shells
# name the same 61 builtins in the same order. It was excluded until 2026-08-02,
# when the last two names osh was missing — `suspend` and then `bind` — landed
# as the refusal and the warning that bash itself gives when there is no job
# control and no line editor. See known-issues TD-OILS-NO-BIND-BUILTIN for the
# parts of `bind` that are still hollow; none of them is a *name*, so none of
# them shows here.
#
# Every probe runs in a subshell so a disabled builtin cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err

echo "=== the full listing names every builtin, in the shell's own order"
( enable | wc -l; echo "  plain     rc=$?" )
( enable -a; echo "  -a        rc=$?" )
( enable -- | wc -l; echo "  --        rc=$?" )
( enable -p | wc -l; echo "  -p        rc=$?" )
( enable -s | wc -l; echo "  -s        rc=$?" )

echo "=== -n turns a builtin off, and lists the ones that are off"
( enable -n; echo "  none      rc=$?" )
( enable -n echo; echo "  off       rc=$?"; enable -n )
( enable -n echo cd; enable -n; echo "  two       rc=$?" )
( enable -n echo; enable echo; enable -n; echo "  back      rc=$?" )
( enable -n echo; echo hi; echo "  runs?     rc=$?" )
( enable -- echo; echo "  --  name  rc=$?" )

echo "=== -s restricts a listing to the special builtins"
( enable -n :; enable -s -n; echo "  special   rc=$?" )
( enable -n echo; enable -s -n; echo "  ordinary  rc=$?" )
( enable -n : echo; enable -s -n; echo "  both      rc=$?" )
( enable -n source; enable -s -n; echo "  source    rc=$?" )
( enable -n test; enable -s -n; echo "  test      rc=$?" )

echo "=== -s with names is only a filter"
( enable -s echo; echo "  echo      rc=$?" )
( enable -s :; echo "  colon     rc=$?" )
( enable -s nosuchbuiltin; echo "  nosuch    rc=$?" )
( enable nosuchbuiltin; echo "  plain     rc=$?" )
( enable -; echo "  -         rc=$?" )

echo "=== -p asks for the form the listing already has"
( enable -n echo; enable -p -n; echo "  -p -n     rc=$?" )

echo "=== an option letter is not read as a name, and the scan is left to right"
( enable -f; echo "  -f alone  rc=$?" )
( enable -nf; echo "  -nf       rc=$?" )
( enable -z; echo "  -z        rc=$?" )
( enable -sz; echo "  -sz       rc=$?" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
