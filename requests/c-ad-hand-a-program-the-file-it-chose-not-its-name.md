# C → A, D — Hand a program the file it chose, not the file's name

**From:** Lane C (`gui/filechooser`). **To:** Lane A (capability transfer on
channels) and Lane D (the POSIX side of an inherited or received file).
**Filed:** 2026-10-05. **Status:** OPEN -- for planning; nothing waits on it.

**In short:** the operator's `design-decisions.md` §1415 has the file
explorer show every program's Open and Save window and hand the program
"only the file chosen". Today the explorer can only *tell* the program the
path, which the program then opens itself -- so the program still needs the
right to open any file it might be told about. The security gain arrives
when the explorer instead hands the program the file, already open, over
the channel they share: then a program needs no access to your files at all
until you choose one. §1415 gives this part to lanes A and D.

## What lane C's side looks like now

`gui/filechooser` (§1463): a program connects to the service
`org.slateos.FileChooser`, sends one request, and reads one reply, which
carries an absolute path. When a file can travel, the reply gains it -- a
version 2 of the protocol, so a chooser and a program of different ages
refuse each other rather than misread.

## What is asked

- **Lane A:** moving an open file -- a file descriptor, or the capability
  behind it -- from one process to another over a channel connection, with
  the rights the sender chooses (read-only for Open; write, create, and
  replace-by-rename for Save, which needs the folder rather than the file).
- **Lane D:** the receiving end as POSIX code sees it: the file arrives as a
  descriptor a program can `read`, `write` and `fstat`, and a Save handed a
  folder can create and rename inside it and nowhere else.
- **Both:** what a program that has been handed nothing may still open of
  your files -- which is the restriction that makes the handing worth doing,
  and which is a policy question as much as a mechanism.

## If this is never done

Programs are told a path and open it themselves, as with any other
dialog today: no weaker than now, and not yet the protection §1415 is for.
