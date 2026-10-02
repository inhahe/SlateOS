## 854. An export of an empty collection is refused, not written

**Date:** 2026-09-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** when you ask one of these apps to save your contacts, calendar or
subscription list to a file and there is nothing in it to save, it refuses and
says so rather than writing an empty file. The reason is that the empty file
would be a perfectly valid one. Opening it later would show nothing, and you
could not tell whether that meant "you saved nothing" or "the save failed" --
and by then the file has usually replaced the copy that had your data in it.

**Where it bites.** `apps/contacts` (`write_vcards`), `apps/calendar`
(`write_ics`), `apps/rssreader` (`write_opml_file`). Each returns a refusal
string -- "No contacts to write", "No events to write", "No feeds to write" --
and creates no file.

**Why this is not the usual "let the user do what they asked".** The three
formats all have a legal empty form. A vCard file with no `BEGIN:VCARD` in it,
an iCalendar file holding only `BEGIN:VCALENDAR`/`VERSION`/`END`, and an OPML
file with an empty `<body>` are all well-formed documents that any reader
accepts and imports as nothing at all. So the empty export is not a visibly
broken file that announces itself. It is a *successful-looking* file, and the
moment it is written over the previous one, the data that was in the previous
one is gone with no error anywhere.

**The asymmetry with reading, which is deliberate.** Reading an empty file is
allowed and reports "added 0 entries". Only writing is refused. Writing is the
operation that destroys something: the path the user picked very often already
holds the only copy of what they are about to overwrite. Reading destroys
nothing, so it needs no protection.

**The alternative that was rejected, and it is a real one.** Write the file and
put "wrote 0 entries" on the status line. That respects the request and still
informs. It was rejected because **the status line is transient and the file is
not**: the sentence is gone at the next keypress, and the file outlives it by
years. The failure this guards against is not misunderstanding at the moment of
saving -- it is discovering months later that the backup is empty. A message
that has already disappeared cannot help there.

**A second alternative, also rejected:** ask for confirmation. There is no
confirmation primitive in `guitk::dialog` yet, and adding one for a case this
rare would train the habit of dismissing it unread, which is worse than either
other option.

**What is given up.** Deliberately clearing a calendar and then exporting it to
overwrite a stale copy on another machine is a legitimate thing to want, and it
is now blocked -- the user has to delete the target file themselves. That is a
real cost, paid by a rare user to protect a common one. If it comes up, the
right fix is a confirmation, not a silent write.

**Recorded in** `known-issues.md` under
`TD-C-FINISHED-SERIALISERS-THAT-NOBODY-COULD-REACH`, which carries the wider
sweep this came out of.
