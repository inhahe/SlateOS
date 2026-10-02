## TD-B-EFIBOOTMGR-MODIFIES-NOTHING (lane B, 2026-09-11) -- FIXED 2026-09-11

**FIXED** by deleting the options, which is what the entry named as the proper
fix. `efibootmgr` is a read-only reporter now: `-v`, `-h`, `-V`. All 32
spellings of the removed options live in one `REMOVED_OPTIONS` const that both
the help text and the parser are built from, so the sentence and the behaviour
cannot drift apart, and re-adding one to the parser without efivarfs writes
behind it fails a test.

**The part the entry did not anticipate, and it is the important part.**
Deleting an option from a parser whose fall-through arm was `_ => {}` does not
refuse it -- it makes it a silent no-op. `efibootmgr -c -L Slate` would have
printed a boot list and exited 0, which reads as success. That is the same lie
with the incriminating sentence removed, and it would have been *harder* to
notice than the fabricated "created Boot0003", because at least that named
which lie it was telling. Every unrecognised argument is an error now, and a
removed one gets a message that says it never wrote anything.

**Two more fabrications in the same crate, found while removing the writes.**
`print_boot_entries` emitted the fixed strings `BootCurrent: 0000` and
`Timeout: 3 seconds` without either variable ever being read -- so a machine
with a 10-second timeout, or one that booted from the network and sets no
`BootCurrent`, was reported wrong in the header of a display whose entire job
is to be that header. Both are read now, and a machine that does not set one
gets no line rather than a plausible wrong one. This is the THIRD defect found
in this crate in one day, and all three are the same shape: a value nobody had
that got printed anyway.

Also fixed in passing, all the same family: an EFI variable whose value is
empty is four bytes (attributes only) and the `> 4` test called it absent; a
label outside the basic plane arrives as a UTF-16 surrogate pair and
`char::from_u32` per code unit rejected both halves, so such characters
vanished from boot labels silently; `BootOrder` naming a `Boot####` that does
not exist is now reported, because the firmware skips it and that is the
symptom somebody runs this to explain; and `efivar` printed "EFI variables are
not supported on this system." to *stdout* and exited 0.


**In short:** `efibootmgr` accepts seven options that change the machine's boot
configuration and performs none of them. It prints `efibootmgr: created
Boot0003` and `efibootmgr: deleted Boot0003` having written nothing at all:
there is no `fs::write`, no `File::create`, no `OpenOptions` anywhere in the
crate. The modifications happen in memory and are then printed as though they
had been applied.

**The options:** `-c/--create`, `-B/--delete-bootnum`, `-a/--active`,
`-A/--inactive`, `-n/--bootnext`, `-o/--bootorder`, `-t/--timeout`.

**Why this is the §1006 case.** design-decisions.md §1006 is the operator's
decision that a command which does not work is deleted, not kept as a stub, and
`userspace/acl`'s `setfacl`/`chacl` were removed under it the same morning for
exactly this — printing "removing all ACL entries from <path>" with zero writes
in 793 lines. This is that, in a tool whose subject is whether the machine
boots.

**It is the second defect found in this crate today and I missed it the first
time.** The earlier repair removed `generate_default_entries`, which invented
two boot entries when efivarfs could not be read, so that the tool stopped
fabricating what it REPORTS. Nobody looked at what it CLAIMS TO WRITE. Reading
a program for one defect family is not reading it.

**The proper fix** is to delete the seven options, their fields on `Options`,
the modification block, their help text and the tests that cover them —
`efibootmgr` becomes a read-only reporter, which is what it actually is. Adding
them back needs efivarfs write support: the immutable attribute has to be
cleared before a variable can be replaced, and `posix` does not expose
`FS_IOC_SETFLAGS` today. That is the condition to reopen this.
