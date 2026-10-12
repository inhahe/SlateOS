### A-UTIMES-SET-THE-CHANGE-TIME-WRONG -- 2026-10-01 -- FIXED the same day (lane A); its nanoseconds OPEN

**In short:** changing a file's times (`touch`, `utimensat`) must set its
change time (`ctime`) to now. ext4 set it to the new modification time, so
`touch -d 2001-01-01 f` back-dated the change time as well. An access-time
change left it alone. memfs never set it.

**Fixed:** ext4's `set_times_ino` stamps the change time now; memfs's
`node_set_times` does too.

**Still open:** ext4's `set_times_ino` writes whole seconds into the inode
core and leaves the extra fields (`i_mtime_extra`, `i_atime_extra`)
alone. A time's nanoseconds and its epoch bits past 2038 are lost, and
the old ones are left in place: a file stamped with a whole second reads
back with the nanoseconds of its previous time. The fix is to write the
extra fields as `write_crtime` writes the creation time.
