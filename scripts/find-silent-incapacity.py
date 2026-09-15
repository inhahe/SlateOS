"""Which programs cannot do the thing they are for, and do not say so?

`find-reachable-fixtures.py` finds programs that say too much -- data invented
and presented as real. It cannot find the opposite failure, and the opposite
failure is common: a program with no fabrication at all, an empty screen, and
nothing explaining why the screen is empty.

`apps/clipmanager` was the case that prompted this. It invents nothing, so the
fixture scanner never flagged it. It also has no capture path: nothing watches
the clipboard and nothing can, so the history is empty and stays empty however
long the window is open. **An empty list under the word "History" is read as a
statement about the user** -- you have not copied anything -- rather than about
the program.

So this asks a different question: **does the crate have any capability at all,
and if not, does it admit that anywhere the user could read it?**

HOW IT DECIDES

A crate is *incapable* if nothing in it touches the outside world: no
`std::fs`, no `std::net`, no `std::process::Command`, no `safeio`, no file
dialog. Those are the four doors anything in `apps/` could come through.

A crate *admits it* if some string literal carries refusal vocabulary --
"cannot", "no ... access", "nothing was", "not saved", "unavailable". The
match is on string literals only, because a comment is not an admission: the
user does not read the source. That distinction is the entire sweep.

WHAT IT CANNOT SEE, stated plainly:

  * A crate can be incapable in a way that does not matter. A calculator needs
    no filesystem and owes nobody an explanation. **The output is a list to
    read, not a list to empty** -- most entries will be fine.
  * A crate can admit one incapacity and stay silent about another. The check
    is per-crate, not per-feature.
  * Vocabulary is a proxy. A window that says "Press Ctrl+O to open a file" in
    an app with no opener passes this check and is still a promise it cannot
    keep -- `apps/pdfviewer` shipped exactly that. Passing here is not a
    clean bill of health.

Report-only, and no --check mode, for the same reason as the fixture scanner:
several entries are legitimate, and a gate would train the next reader to
silence it rather than read it.

Usage:  python scripts/find-silent-incapacity.py [--roots=apps]
"""

import pathlib
import re
import sys

# Anything that reaches outside the process.
CAPABILITY = re.compile(
    r"std::fs\b|std::net\b|std::process::Command|safeio::|FileDialog|list_directory"
    r"|std::env::var|read_dir\b"
)

# Refusal vocabulary, matched inside string literals only.
# Deliberately wide. The first draft matched only "cannot" and a few cousins,
# and reported `apps/diskanalyzer` and `apps/pdfviewer` as silent -- both of
# which admit clearly, in words it had not thought of ("Could not scan: {err}",
# "none can be opened"). A vocabulary list that is narrower than the language
# produces false accusations, and a tool that accuses honest code is a tool the
# next reader learns to skip.
ADMITS = re.compile(
    r"cannot|can't|could not|couldn't|none can|no way to"
    r"|no .{0,30}(access|source|installed|available|configured|reader|driver|service)"
    r"|nothing (was|is|has|here|can)|not (saved|kept|yet|able|connected|implemented|examined)"
    r"|unavailable|unimplemented|under construction|has no |never (fetched|examined|contacted|sent)",
    re.I,
)

STRING_LIT = re.compile(r'"((?:[^"\\]|\\.)*)"')


def main():
    roots = ["apps"]
    for arg in sys.argv[1:]:
        if arg.startswith("--roots="):
            roots = [r for r in arg.split("=", 1)[1].split(",") if r]
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2

    silent, capable, admitting = [], 0, 0
    for root in roots:
        base = pathlib.Path(root)
        if not base.is_dir():
            print(f"no such directory: {root}", file=sys.stderr)
            return 2
        for crate in sorted(p for p in base.iterdir() if (p / "src").is_dir()):
            src = "".join(
                f.read_text(encoding="utf-8", errors="replace")
                for f in sorted((crate / "src").rglob("*.rs"))
            )
            # Test code is not the shipping program.
            prod = src
            marker = "\n#[cfg(test)]\nmod tests"
            if marker in prod:
                prod = prod[: prod.index(marker)]

            if CAPABILITY.search(prod):
                capable += 1
                continue

            said = [m.group(1) for m in STRING_LIT.finditer(prod) if ADMITS.search(m.group(1))]
            if said:
                admitting += 1
                continue

            # How loudly does it talk about doing things it cannot do?
            verbs = len(
                re.findall(
                    r"\b(save|load|open|export|import|scan|connect|record|capture|sync|fetch)\b",
                    prod,
                    re.I,
                )
            )
            silent.append((crate.name, verbs))

    silent.sort(key=lambda kv: -kv[1])
    print(
        f"{len(silent)} crate(s) reach nothing outside the process and say so nowhere\n"
        f"({capable} have a capability, {admitting} are incapable and admit it)\n"
    )
    print("  the second column counts verbs like save/open/scan/connect in")
    print("  production code -- a rough measure of how much the program claims")
    print("  to do, and therefore how loudly its silence reads\n")
    for name, verbs in silent:
        print(f"    {name:<20} {verbs}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
