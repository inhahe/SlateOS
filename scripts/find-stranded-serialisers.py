"""Which finished serialisers can nobody reach?

The third of the trio.

  find-reachable-fixtures.py   programs that say too much
  find-silent-incapacity.py    programs that say too little
  this one                     work that is finished and cannot be used

`apps/spreadsheet` is the case that prompted it. `Sheet::export_csv` and
`Sheet::import_csv` were both written and both tested, and neither could be
called -- the module's own doc said so, in as many words: "nothing can reach
them: they take and return a `String`, and this program has no file dialog".
That is an accurate description of a consumer with no producer, and it had sat
there as a *description* rather than as a task. **The serialiser was the hard
part and it was already finished; what was missing was twenty lines of
picker.**

So this asks: does the crate define a function that turns its data into bytes
or text (or back), while having no way to read or write a file?

HOW IT DECIDES

A *serialiser* is a function whose name begins `export_`, `import_`,
`serialize_`, `deserialize_`, `to_`, `from_`, `save_` or `load_` **and** whose
signature mentions `String`, `&str`, `Vec<u8>` or `&[u8]`. The second half is
what keeps `to_screen`, `from_index` and `to_radians` out: a coordinate
transform is not a serialiser, and the name alone cannot tell you.

A crate *has a door* if it mentions `FileDialog`, `safeio::`, `std::fs::read`
or `std::fs::write` anywhere outside its tests.

WHAT IT CANNOT SEE:

  * A serialiser may be reachable by some route other than a file -- a
    clipboard, an IPC message, an argument. None of those exist in `apps/` yet,
    which is why the check is worth making now and will need revisiting.
  * It cannot tell a finished serialiser from a stub with the right shape.
    `export_ics` might be three lines returning an empty string. **Read the
    function before promising anyone a door.**
  * A format may be a bad idea even when implemented. Reaching it is not
    automatically an improvement.

Report-only. The count is not a number to drive to zero -- some of these are
one line of a format nobody needs -- but every entry is finished work that
nobody can currently use, which is an unusual and cheap kind of lead.

Usage:  python scripts/find-stranded-serialisers.py [--roots=apps]
"""

import pathlib
import re
import sys

SERIALISER = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+"
    r"((?:export|import|serialize|deserialize|save|load|to|from)_[a-z_0-9]+)"
    r"\s*(?:<[^>]*>)?\s*\(([^{;]*)",
    re.M,
)
BYTES = re.compile(r"\bString\b|&\s*str\b|Vec\s*<\s*u8\s*>|&\s*\[\s*u8\s*\]")
DOOR = re.compile(r"FileDialog|safeio::|std::fs::read|std::fs::write")


def main():
    roots = ["apps"]
    for arg in sys.argv[1:]:
        if arg.startswith("--roots="):
            roots = [r for r in arg.split("=", 1)[1].split(",") if r]
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2

    stranded, doored = [], 0
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
            prod = src.split("\n#[cfg(test)]\nmod tests")[0]
            if DOOR.search(prod):
                doored += 1
                continue

            names = []
            for m in SERIALISER.finditer(prod):
                name, sig = m.group(1), m.group(2)
                # The signature proves it moves text or bytes. A name cannot.
                tail = prod[m.start() : m.start() + 400]
                if BYTES.search(sig) or BYTES.search(tail.split("{", 1)[0]):
                    names.append(name)
            names = sorted(set(names))
            if names:
                stranded.append((crate.name, names))

    total = sum(len(n) for _, n in stranded)
    print(
        f"{total} serialiser(s) with no way to reach a file, "
        f"across {len(stranded)} crate(s)\n"
        f"({doored} crates already have a door)\n"
    )
    print("  READ THE FUNCTION before promising anyone a door: this cannot")
    print("  tell a finished serialiser from a stub with the right shape.\n")
    for name, fns in sorted(stranded, key=lambda kv: (-len(kv[1]), kv[0])):
        print(f"    {name:<18} {', '.join(fns)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
