## TD-C-THE-INSTALLER-RECORDS-A-GRUB-PATH-NOTHING-EVER-READS -- 2026-09-16 -- **FIXED 2026-09-25**
**Status:** FIXED 2026-09-25 by lane E -- both dead fields deleted as this entry prescribes (`grub_cfg_path` and `custom_dir`, with the local that only fed them); the record was serialised nowhere, and `parse_grub_config` itself has only its two tests as callers.

**In short:** the installer builds a record of the boot loader's configuration
and puts the path of its config file in it. Nothing anywhere reads that field.
It is written, stored, and never consulted.

**Date:** 2026-09-16. **Lane:** C.

**Where.** `apps/installer/src/grub.rs` -- `pub grub_cfg_path: String` declared
at line 108, assigned at line 480 from
`install.config_path.to_string_lossy().into_owned()`. Those two lines are the
*only* mentions of the name in `apps/` and `gui/` combined.

**How it was found, which is the interesting part.** Not by looking for dead
code. The lossy-decode checker flagged line 480 as a path being flattened to
text, and the triage question -- *does anything downstream use this as a
path?* -- has the answer "nothing downstream uses it at all". The byte-safety
question was unanswerable because the field has no consumer to ask about.

That makes it design-decisions 856 again, from an unexpected direction: *a
settings page is built when something obeys it, not when something stores it.*
Here a struct field stores a value nothing obeys, and the only reason anyone
looked is that it stored it **wrongly**. A correct-looking dead field would
still be sitting there.

**The proper fix** is to delete the field, not to make its conversion
byte-correct -- fixing the conversion would make a dead field *defensibly*
dead, which is worse, because the next reader would find a careful-looking line
and assume it mattered. If a consumer is intended (an installer that later
edits the config it wrote), the field should be a `PathBuf` and arrive with
that consumer.

**`custom_dir`, on the next line, is dead the same way -- and proving it takes
care.** A grep for the name returns thirteen hits, which reads as "thoroughly
used". They belong to three different things: a local variable, this dead
`String` field, and a `custom_dir: PathBuf` field on a *different* struct at
line 553. All three real reads (`self.custom_dir.join(...)`,
`.is_dir()`) are that third one. `GrubConfig`'s own field has none, exactly
like `grub_cfg_path`.

**Which makes the file more interesting than a dead-code entry.** The correct
model is already in it, eleven lines below the wrong one: the same concept held
as a `PathBuf` and used as a path, beside a `String` flattened out of a path
and used for nothing. Nobody has to decide what the right shape is here -- it
has already been written, once, and the surviving copy is the one with a
consumer. That is the rule from design-decisions 857 in its natural habitat:
**when one of two models has a consumer and the other does not, the one with
the consumer is the real one.**

The other three `GrubConfig` fields -- `timeout`, `default_entry`,
`os_prober_enabled` -- each have one or two readers, so the struct as a whole
is consumed. These two are the odd ones out, not a wholly unused type.

**Not done now** because deleting two `pub` fields from the installer wants a
look at whether the record is serialised anywhere first, and because the fix is
a deletion whose value is in being done deliberately rather than as a
by-product of a byte-safety sweep.
