/* SPDX-License-Identifier: MIT */
/*
 * The headers of Linux's that assume kernel mode, replaced for a driver
 * process (design-decisions 1578 rule 1, 1580).
 *
 * This file is put before every C file the layer compiles -- Linux's own and
 * the layer's -- with `-include`, after Linux's own forced includes. Each
 * header it includes defines the include guard of the Linux header it
 * replaces and then what that header declares, so when Linux code includes
 * the original, the original's guard is already defined and it adds nothing.
 * Linux's files are never edited, and nothing here depends on where an
 * include path puts them. `scripts/linux-tree.sh` checks that each guard named
 * here is the guard the pinned tree's header really has: a guard that no
 * longer matches would let both versions in, or neither.
 *
 * Each shadow names its Linux header and its guard on a line of its own:
 *     SHADOWS <path under the Linux tree> <guard>
 * which is what the check reads.
 */
#ifndef SLX_SHADOW_H
#define SLX_SHADOW_H

#ifndef __ASSEMBLER__
#include "irqflags.h"
#include "bug.h"
#endif

#endif /* SLX_SHADOW_H */
