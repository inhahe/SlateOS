### [F] Old-style JPEG TIFFs: three corners of libtiff's reading not modelled -- 2026-09-25

**Status:** OPEN — lane F's; left deliberately, each needing input built to
reach it.

**In short:** old-style JPEG TIFFs decode exactly as libtiff decodes them
except in three situations only a file made for the purpose reaches, where
this could refuse a picture libtiff shows, or show one libtiff refuses.

**Where.** `gui/imagecodec/src/tiff/ojpeg.rs` (its module notes).

1. **When input is asked for.** libtiff hands libjpeg a block 2048 bytes at
   a time, and libjpeg-turbo's Huffman decoder has a fast path that asks for
   nothing while a piece still holds 512 bytes for each block of the MCU.
   What is decoded is the same; when the source is next asked is not. It
   shows only if the input ends hard (no strip after the data), with no
   restart interval, within a few bytes of the last bits the scan needs:
   then one of the two fails and the other does not.
2. **Where a session starts.** libtiff restarts a plane from its first data
   unless its input already stands at that file offset, and then carries on
   from where it is -- the same thing, unless one file range belongs to two
   blocks (strips of different planes overlapping) and the last session
   stopped at a piece boundary exactly there.
3. **A BigTIFF table offset past 2^63** is a failed seek in libtiff, which
   then reads the table from wherever its file position was; here there is
   no table and the file is refused.

**The proper fix.** Model libtiff's input exactly: its 2048-byte pieces and
how much of each libjpeg has taken (which needs libjpeg-turbo's fast path,
since it takes bytes differently), its `in_buffer_file_pos_log`, and its
file position through every read. No writer's file reaches any of them, and
20,000 fuzzed files found none.
