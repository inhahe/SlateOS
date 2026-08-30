"""Run `rustfmt` over a file a generator just wrote.

Why a generator has to do this at all
-------------------------------------

The eleven scripts in this directory emit `.rs` files, and what they emit is
whatever string formatting produced -- not what `rustfmt` would produce. That
was invisible for as long as nobody ran the formatter over `gui/font/src`, and
it stopped being invisible the moment somebody did: `cargo fmt -p osfont`
rewrites eight generated files, and the next regeneration rewrites them back.
The two would then take turns, and every diff in between would show a thousand
lines of table churn wrapped around whatever real change was being made.

CLAUDE.md's rule is "rustfmt defaults, no manual formatting overrides", and
there is no `rustfmt.toml` in this tree, so a generated file has no more claim
to its own formatting than a hand-written one. The fix is therefore not to
exempt these files from the formatter but to make regeneration produce what the
formatter produces, so that running either one is a no-op after the other.

There is a second reason, less obvious and more valuable: the repository's
pre-push hook checks every `.rs` file a push touches, and `rustfmt` follows
`mod` declarations. So a one-line edit to `gui/font/src/lib.rs` -- the crate
root, which declares all of these modules -- is checked as the whole crate,
generated tables included. Without this, no change to `lib.rs` can be pushed
without either reformatting the tables or bypassing the gate.

What this does not promise
--------------------------

`rustfmt` is not required to be installed, and a machine without it should
still be able to regenerate a table -- the alternative is a generator that
fails for a reason having nothing to do with the data it is generating. So a
missing `rustfmt` is a warning on stderr, not an error. It is the pre-push gate
that ultimately enforces the formatting; this is what keeps that gate from
having anything to complain about.

A `rustfmt` that *fails* is different, and is reported loudly: on a generated
file the only plausible cause is output that does not parse, which means the
generator emitted broken Rust and the file is worse than unformatted.
"""

import shutil
import subprocess
import sys

# Every crate in this tree is edition 2024, and `rustfmt` invoked directly (as
# opposed to through `cargo fmt`) defaults to 2015, under which `gen`, `async`
# and a good deal else parse differently. Passing it explicitly is what makes
# this agree with `cargo fmt -p osfont` and with the pre-push hook, which
# passes the same flag for the same reason.
EDITION = "2024"


def rustfmt(path):
    """Format `path` in place, and say so. Returns True if it ran."""
    exe = shutil.which("rustfmt")
    if exe is None:
        print(
            f"warning: rustfmt not found; {path} written unformatted. "
            "`cargo fmt -p osfont` before committing it.",
            file=sys.stderr,
        )
        return False

    result = subprocess.run(
        [exe, "--edition", EDITION, path],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        # Loud, and fatal to the caller's exit status, because the realistic
        # cause is Rust that does not parse.
        raise SystemExit(
            f"rustfmt failed on {path} (exit {result.returncode}).\n"
            f"{result.stderr.strip()}\n"
            "A generated file that rustfmt cannot parse does not compile "
            "either; fix the generator's output."
        )
    return True
