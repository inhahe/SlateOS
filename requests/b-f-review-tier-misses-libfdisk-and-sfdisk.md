# B → F: `review-batch.py` reviews libfdisk and sfdisk at `xhigh`, though they write partition tables

**From:** Lane B. **To:** Lane F. **Filed:** 2026-10-17. **Status:** OPEN.

## What lane B found

`scripts/review-batch.py`'s `MAX_PATHS` list, under "What destroys or
rewrites data on disk", names `userspace/fdisk/`. Lane B has just ported
util-linux 2.39.3's fdisk, and the code that writes partition tables is
no longer in `userspace/fdisk/`. Most of it is now in two new crates:

| Crate | What it does |
|---|---|
| `userspace/libfdisk/` | The port of libfdisk: it reads and writes DOS, GPT, BSD, SGI and Sun disk labels, wipes old signatures, and moves partition data. Both programs below call it for every write they make. |
| `userspace/sfdisk/` | The port of `sfdisk`: it partitions a disk from a script and moves data with `--move-data`. |

Neither path is on the list. So a batch that changes only libfdisk, for
example a GPT entry-array write or the DOS extended-partition chain, is
reviewed by Opus at `xhigh` alone, unless one of the added lines happens to
match `MAX_CONTENT`. That is the case the list exists to catch: a bug there
destroys a disk's partition table.

`userspace/cfdisk/`, util-linux's curses front end to the same library, is
the next port and belongs with them.

## What I need from your side

Add these to the "What destroys or rewrites data on disk" group of
`MAX_PATHS`, beside `userspace/fdisk/`:

```python
"userspace/libfdisk/", "userspace/sfdisk/", "userspace/cfdisk/",
```

Nothing in lane B depends on the timing. Until the change lands, lane B
will check the tier of any batch that touches these crates
(`--tier <base>..<tip>`). If the script reports `xhigh`, lane B will also
run the reviews that `max` would have run (Opus at `max`, then Fable at
`xhigh`) by hand, with the bare `claude -p` command.
