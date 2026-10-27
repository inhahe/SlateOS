"""Facts about one row of `bench/boot-history.jsonl` that more than one script reads.

`boot-history.py` writes the rows and reads them back for its statistics;
`check-boot-skips.py` reads them to judge allowlist entries whose condition is
a property of the boot. A rule for reading a field lives here, once, so the
two cannot disagree about a row -- and so the gate does not have to execute
all of `boot-history.py` to ask one question of a row.

Kept to plain functions of a row with no imports beyond the standard library:
a gate that imports this must not be taken down by anything else.
"""

from __future__ import annotations

__all__ = ["cpus_of"]


def cpus_of(rec: dict) -> int | None:
    """How many CPUs a record's boot ran on, `None` when it cannot say.

    A row without the field predates it, and for an ordinary boot that is
    not a guess but a fact of the harness: until `--smp` existed
    (2026-10-09) boot-test.sh started QEMU with no `-smp`, which is one CPU.
    An *experiment* row is the exception -- its `QEMU_EXTRA` could have
    named any count -- so one without the field cannot say. A row with the
    field carries what QEMU was given, or `null` when the recorder was not
    told.
    """
    if "cpus" not in rec:
        return None if rec.get("experiment") else 1
    val = rec["cpus"]
    return val if isinstance(val, int) and not isinstance(val, bool) else None
