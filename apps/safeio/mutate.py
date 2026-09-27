"""Mutation test for `safeio`'s no-overwrite write, `write_new_atomically`.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Only the newest function is swept here; the rest of the crate predates this
table and is covered by its own tests.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a name in use is renamed over",
        "    write_new_with(path, contents, |from, to| fs::hard_link(from, to))",
        "    write_atomically(path, contents)",
        ["a_name_in_use_is_refused_and_left_as_it_was", "a_directory_at_the_name_is_refused"],
    ),
    (
        "the temporary is left as a second name",
        "        let _ = fs::remove_file(&tmp_path); // Best effort; a stray second name for a whole file loses nothing.",
        "",
        ["a_free_name_is_written_and_nothing_else_is_left"],
    ),
    (
        "without hard links the claim is not exclusive",
        "        .create_new(true)\n"
        "        .open(target)?;",
        "        .create(true)\n"
        "        .open(target)?;",
        ["without_hard_links_the_name_is_still_claimed_exclusively"],
    ),
    (
        "without hard links nothing is written",
        "    } else if let Err(e) = claim_then_rename(&tmp_path, path) {",
        "    } else if let Err(e) = Err::<(), io::Error>(io::Error::from(io::ErrorKind::Unsupported)) {",
        ["without_hard_links_the_name_is_still_claimed_exclusively"],
    ),
    (
        "a refused write leaves its temporary",
        "        let _ = fs::remove_file(&tmp_path); // Best effort; the publish error is the one worth reporting.",
        "",
        ["a_name_in_use_is_refused_and_left_as_it_was"],
    ),
    (
        # Windows reports an exclusive create over a directory as "access
        # denied"; Linux says "exists" by itself, so on Linux this survives.
        "a directory at the name is reported as a permission problem",
        "        if e.kind() != io::ErrorKind::AlreadyExists && fs::symlink_metadata(path).is_ok() {",
        "        if false {",
        ["a_directory_at_the_name_is_refused"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "safeio", timeout=300, only=only))
