### [F] Lossless JPEG is refused though libjpeg-turbo decodes it -- 2026-09-25

**Status:** FIXED 2026-09-25. `gui/imagecodec/src/jpeg/lossless.rs` ports the
three files below; 56 fixtures and 20,000 mutants agree with libjpeg-turbo
(design-decisions §1318, "Lossless JPEG").

**In short:** a JPEG coded losslessly (`SOF3`) -- used by medical imaging and
some scientific instruments, almost never for photographs -- is refused
(`ImageError::Unsupported`), where libjpeg-turbo 3, which the rest of the
JPEG decoder is a port of, decodes it at up to 8 bits a sample. Nothing
else about JPEG is affected.

**Where.** `gui/imagecodec/src/jpeg/decompress.rs` (`start`, which refuses a
lossless frame).

**The proper fix.** Port libjpeg-turbo's lossless decompressor -- `jdlossls.c`
(prediction and undifferencing), `jddiffct.c` (the difference buffer
controller) and `jdlhuff.c` (its Huffman decoder) -- as the rest was
ported (design-decisions §1318), and fuzz it against the same oracle.
