### [E] The Font Manager lists a family's per-style names as families of their own -- 2026-10-10

**Status:** OPEN -- waits on lane C
(`requests/e-c-the-font-index-could-say-which-name-is-a-faces-family.md`).

**In short:** Many font families give each style's file two family names: the
family itself ("Noto Sans") and an older name for that one style ("Noto Sans
SemiBold"). The Font Manager lists every one of those names as a family. Noto
Sans therefore shows as dozens of one-style families beside itself, and
removing one says that another "goes with" it when they are the same file.

**Where:** `apps/fontmanager/src/library.rs` (`group`), which works from
`guitk::fontdb::FaceInfo::display`. That field does not say which name is the
family. `library_tests::a_file_another_family_shares_is_named` pins the
current behaviour.

**Fix:** once the index records each face's own family (the request), group
by it, and keep the other name as one the family answers to.
