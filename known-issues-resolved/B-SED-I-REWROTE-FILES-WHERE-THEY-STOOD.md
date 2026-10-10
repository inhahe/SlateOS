## B-SED-I-REWROTE-FILES-WHERE-THEY-STOOD (lane B, 2026-10-03)

**Status:** FIXED 2026-10-03 (lane B); boot-tested on main (394c97655, published as 109a26eec).

**In short:** `sed -i` is meant to replace a file with its edited copy. GNU
sed writes the copy to a new file beside the original and renames it into
place, so the name always holds a complete file. Ours built the edit in memory
and then overwrote the original where it stood -- so a disk that filled
part-way through the write left the file truncated and the text gone. The same
difference changed what an edit means in six other ways (below), and
`--follow-symlinks` was accepted and ignored.

### What was wrong, measured against GNU sed 4.9

| | ours | GNU |
|---|---|---|
| the disk fills during the write | the original truncated first (`fs::write`), then written as far as the space went: a bigger edit loses the rest of the text, status 2 | `couldn't write 99 items to ./sedXXXXXX: No space left on device`, 4; the original untouched, the temporary removed |
| `sed -i ... g` where `hard` is a second name for `g` | both names edited | `g` is a new file; `hard` keeps the old text |
| `sed -i ... link` where `link -> target` | `target` edited, `link` still a link | `link` replaced by an edited file; `target` untouched |
| `sed -i --follow-symlinks ... link2` | (the same, by accident) | `target`, at the end of the links, edited |
| `sed -i ... ro` (mode 444, writable directory) | `couldn't write ro: Permission denied`, 2 | edited, still 444 |
| `sed -i ... ud/x` (directory 555, file writable) | edited | `couldn't open temporary file ud/sedXXXXXX: Permission denied`, 4 |
| `sed -i.bak ... g` | `g.bak` a copy | `g.bak` the original itself, renamed: same inode, same times |
| `sed -i.bak ... link` | `link.bak` a copy of `target` | `link.bak` the link itself, renamed |
| `sed --follow-symlinks -n F link2` | `link2` | `target` |
| `sed --follow-symlinks p nosuch` | `can't read nosuch`, 2 | `couldn't readlink nosuch: No such file or directory`, 4 |

### The fix

`sed.rs`: `InPlace` is GNU's in-place `open_next_file` and `closedown` -- refuse
a terminal or anything but a regular file; `mkostemp` on `DIR/sedXXXXXX` with
mode 0600; stream the edit into it as any output is streamed (so its write
failures read as GNU's do); `fchown` to the original's owner and group (or its
group alone), then the original's mode; close it, checked; rename the original
to the backup if there is one; rename the new file over the name. Until that
last rename the temporary is GNU's `G_file_to_unlink`, removed by every exit.
`follow_symlink` is GNU's: the last component replaced textually by the link's
contents until it is not a link, a link that cannot be read or a loop fatal;
the result is what `F` names and, under `-i`, what is replaced.

Not reproduced: ACLs and SELinux contexts (gnulib's `copy_acl` copies an ACL
where there is one; here the mode is all that is copied).

### Verified

`scripts/sed-diff.sh`: 598 passed, 0 differed, 6 differ on purpose (was 574)
-- `fs_case`, which compares the whole directory afterwards (contents, modes,
link counts, link targets, anything left behind), 22 cases; and `full_case`,
two edits on a 16 KiB tmpfs in a user and mount namespace. Unit tests: `sed`
108 (2 new: a hard link keeps the old text; a backup is the original).
