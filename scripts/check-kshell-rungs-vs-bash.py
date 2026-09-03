#!/usr/bin/env python3
"""Check rung 115's assertions against real bash, exactly as written.

`check-kshell-pipeline-vs-bash.py` pins the *rules*; this pins the *literals
typed into the rung*, which is a different risk: a rule can be right and the
case transcribed into Rust can still be a different string than the one that
was measured, because both Rust and Python re-escape backslashes on the way
in.  Requires WSL; see `bashprobe.py`.

So each entry below carries the Rust source text as well.  If the two ever
disagree about how many backslashes there are, that is visible here rather
than as a mysterious boot-test failure.

Only the cases that are questions about *bash* are listed.  `expand_braces`
is our own stage (it runs before word splitting and preserves text, which
bash has no equivalent of), so its assertions are not checkable here and are
deliberately absent rather than faked.
"""
import sys

import bashprobe


def W(*words):
    return [w.encode() for w in words]


# (rust source as typed in kshell.rs, the actual bytes, expected words)
CASES = [
    # --- remove_quotes: one word in, one word out. ---------------------
    (r'"\"it\'s fine\""', '"it\'s fine"', W("it's fine")),
    (r'"a\\ b"', "a\\ b", W("a b")),
    (r'"\"C:\\dir\""', '"C:\\dir"', W("C:\\dir")),
    (r'"\"say \\\"hi\\\"\""', '"say \\"hi\\""', W('say "hi"')),
    (r'"\"a\\\\b\""', '"a\\\\b"', W("a\\b")),
    (r'"\'a\\\\b\'"', "'a\\\\b'", W("a\\\\b")),
    (r'"\'a\'\\\'\'b\'"', "'a'\\''b'", W("a'b")),
    (r'"a\'b\'c"', "a'b'c", W("abc")),

    # --- split_words: arity and content. -------------------------------
    (r'"a\\ b"', "a\\ b", W("a b")),
    (r'"\"a b\" c"', '"a b" c', W("a b", "c")),
    (r'"a b  c"', "a b  c", W("a", "b", "c")),
    (r'"x\'y z\'w"', "x'y z'w", W("xy zw")),
    (r'"\"a\'b\" c"', '"a\'b" c', W("a'b", "c")),
]


# --- rung 117: awk, whose oracle is awk and not bash. ----------------------
#
# `awk_split_print_args` is internal, so what is checkable from outside is
# what real awk *prints*.  Both cases are about the same disagreement between
# the two languages: awk honours `\"` inside a string and treats `'` as an
# ordinary character, where the shell does neither.  The second case is the
# control -- it is what would break if the shared shell scanner were
# substituted here, which is why the rung refuses that substitution.
#
# (program body, expected stdout, what it pins)
AWK_CASES = [
    (
        r'{ print "a\"b", "c" }',
        'a"b c\n',
        "an escaped quote does not close the string, so the comma separates",
    ),
    (
        "{ print \"it's\", \"x\" }",
        "it's x\n",
        "an apostrophe is data, so the comma still separates",
    ),
    (
        r'{ print "a,b" }',
        "a,b\n",
        "a comma inside a string is not a separator",
    ),
]


def check_awk():
    """Ask real awk what rung 117's cases print."""
    fails = 0
    print("\n--- rung 117, against real awk ---")
    for body, want, why in AWK_CASES:
        # The program reaches awk through a quoted here-doc written to a file
        # and then read with `-f`, so neither bash nor the argv transport can
        # reinterpret a backslash on the way in -- the same hazard this whole
        # file exists to rule out, arriving through a different door.
        script = (
            b"tmp=$(mktemp)\ncat > \"$tmp\" <<'AWK_EOF'\n"
            + body.encode()
            + b"\nAWK_EOF\necho x | awk -f \"$tmp\"\nrm -f \"$tmp\"\n"
        )
        r = bashprobe.run(script)
        got = r.stdout.decode("utf-8", "replace")
        ok = r.returncode == 0 and got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} awk {body}")
        print(f"       awk ={got!r}")
        if not ok:
            print(f"       rung={want!r}   <-- the rung is wrong ({why})")
    return fails


def main():
    bashprobe.assert_transport_is_faithful()
    print("transport verified faithful\n")
    fails = 0
    for rust_src, line, want in CASES:
        got = bashprobe.words(line)
        ok = got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} rust {rust_src:26} = {line!r}")
        print(f"       bash={got!r}")
        if not ok:
            print(f"       rung={want!r}   <-- the rung is wrong")
    fails += check_awk()
    print(f"\n{fails} rung assertion(s) disagree with the reference tool")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
