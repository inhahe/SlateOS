### [E] The screenshot tool's first save can replace a file that took its name a moment earlier -- 2026-09-25
**Status:** FIXED 2026-09-26 -- `apps/screenshot/src/main.rs`
(`write_new_file`, `write_first_free`, `save_names`). A new capture now claims
each name with `safeio::write_new_atomically`, in the same step as it writes
it: a name taken since the look is passed over like one seen taken, and when
all 9,999 names are taken the save fails with "every name up to … is in use"
instead of replacing the first. Saving a capture again over its own file keeps
`write_atomically`. A folder that cannot be written fails at the first name
with its own reason. Five tests; `apps/screenshot/mutate.py` (15 rows) covers
the save path. Still unreachable in the shipped binary until the compositor
offers a framebuffer read (`CANNOT_CAPTURE`).

**In short:** a new screenshot picks a file name nothing holds, then writes it
with `safeio::write_atomically`, which replaces whatever is at the name. A file
another program creates between the check and the write is replaced -- a
window of microseconds here, where the media converter's was minutes (it
plans a queue of names). After 10,000 taken names it falls back to the plain
name on purpose, which overwrites.

**The proper fix:** try the names in order with `safeio::write_new_atomically`
(2026-09-25), which refuses a name in use atomically, until one is claimed --
and never fall back to replacing. Saving a capture again over its own file
keeps `write_atomically`, which is right for that.
