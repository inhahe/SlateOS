## 1223. The recycle bin is a view over a folder, and Empty erases what it showed

**Date:** 2026-09-27
**Lane:** E
**Decided by:** Claude (autonomous), answering lane C's
`c-e-the-recycle-bin-icon-has-nowhere-to-open`

**In short:** the file manager shows the recycle bin in its file pane, in place
of the folder, when the desktop's bin icon runs `explorer --recycle-bin` or the
sidebar's "Recycle Bin" is clicked. It is not a folder to walk into: Back,
Escape or going anywhere leaves it for the folder underneath, as it was. And
"Empty recycle bin" erases the items the bin *showed* when it asked -- not
something that arrived while the question was on screen.

| Call | Chosen | The other way, and why not |
|---|---|---|
| What the bin is on screen | a view of `RecycleBin::list`, over the folder | a folder at `~/.recycle`: its entries are id-named folders each holding a file called `data`, and a paste into it would put a file where the bin cannot see it |
| Where Back goes | out of the bin, to the folder it was opened over | a place in the history like a folder: the history holds paths, the bin is not one, and a `Location` type through every navigation path is a large change for no visible gain |
| What Empty erases | the entries listed when it asked, by id | everything in the bin at the moment of confirming (what other systems do): an item recycled while the dialog was up -- a second window deleting -- would be erased without ever having been shown |
| Double click on a row | nothing | restore it: a gesture that means "look" should not move a file |
| A damaged entry (unreadable record) | listed, erasable, not restorable; Restore is off and says why | hidden: its space would be unaccountable and un-freeable |
| Folder and link sizes | "Folder" and "Link" in the Size column | a folder's own byte count, which is not what a size means; or its contents' total, which means walking every recycled tree each time the bin is listed |

**Also decided here, in `recyclebin`:** an entry id must be one plain name --
`delete("../Documents")` is refused before anything is joined to it -- and the
bin never looks through a link, whether listing (a link planted in the bin is
not an entry), describing (a recycled link is `is_link`, not the folder it
names) or measuring.
