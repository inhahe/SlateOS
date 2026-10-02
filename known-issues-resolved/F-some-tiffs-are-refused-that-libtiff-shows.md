### [F] Some TIFFs are refused that libtiff shows -- 2026-09-25

**Status:** FIXED 2026-09-25 -- PixarLog (`gui/imagecodec/src/tiff/pixarlog.rs`)
was the last; every compression libtiff's reader decodes now decodes here
(design-decisions §1317). The original report follows.

**In short:** TIFFs compressed with PixarLog are refused
(`ImageError::Unsupported`) though libtiff reads them. It is a rare, single
vendor's format. (`YCbCr` and CIELab samples, fax, JPEG, old-style JPEG,
NeXT, ThunderScan and SGI LogLuv were on this list; they decode now.)

**Where.** `gui/imagecodec/src/tiff/read.rs` (`run_codec`) and `rgba.rs`
(`pick_contig`, `pick_separate`, `begin`).

**The proper fix.** Port the rest of libtiff's reader, as the first stages were
(design-decisions §1317): `tif_pixarlog.c`, whose tables are made with
glibc's `exp` and `log` and must come out as glibc's do -- generated from
glibc, as CIELab's were. Fixtures from the same libtiff oracle.
