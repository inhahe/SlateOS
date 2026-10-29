// SPDX-License-Identifier: MIT
/*
 * Linux's direct map, as a driver process has it (design-decisions 1584).
 *
 * Linux code turns an address of kernel memory into its page and back --
 * __pa(), virt_to_page(), page_address() -- by x86's arithmetic over the
 * direct map: memory from page_offset_base on, a struct page for each
 * 4 KiB of it in an array at vmemmap_base. Here the direct map is the
 * process's heap (the kit's buddy region, slx_heap), its "physical"
 * addresses offsets into it, and the page array a region beside it that
 * reads 0 and is backed only where touched, so that only the entries of
 * pages handed out cost memory. Linux's own arithmetic then holds for every
 * block kmalloc hands out. An address outside the heap -- the process's own
 * data, a task's stack, a BAR's mapping -- has no page.
 *
 * Nothing reads a page's entry before the page is handed out, so each
 * allocation sets what its pages say (lx_mm_mark_slab): the entries of
 * pages never handed out stay zero, untouched.
 */
#include <linux/mm.h>
#include <linux/page-flags.h>
#include <linux/init.h>

#include "slx.h"
#include "lx_emul.h"

unsigned long page_offset_base __ro_after_init;
unsigned long vmemmap_base __ro_after_init;
/* What __pa() adds for an address in the kernel's own image; there is none. */
unsigned long phys_base;

int lx_mm_init(void)
{
	unsigned long base, bytes;
	void *memmap;

	slx_heap(&base, &bytes);
	if (!bytes)
		return -ENOMEM;
	memmap = slx_map_lazy(bytes / PAGE_SIZE * sizeof(struct page));
	if (!memmap)
		return -ENOMEM;
	page_offset_base = base;
	vmemmap_base = (unsigned long)memmap;
	return 0;
}

/*
 * A driver process has no vmalloc area: every address Linux code holds is
 * the heap's, the window's, or the process's own, none of them vmalloc
 * memory (whose DMA Linux refuses, it being no direct-map memory).
 */
bool is_vmalloc_addr(const void *x)
{
	return false;
}

/*
 * The `size` bytes kmalloc() hands out at `p` are slab memory, as Linux's
 * own are: their pages say so (PageSlab), which is how Linux code tells an
 * object it may kfree() from one it took elsewhere -- kernel/resource.c's
 * free_resource() among them (lx_mm_mark_slab). Whole pages handed out
 * otherwise -- coherent DMA memory (lx_dma.c) -- are no slab's, and say
 * that, whatever their pages last held (lx_mm_mark_pages).
 */
void lx_mm_mark_pages(const void *p, size_t size)
{
	struct page *page = virt_to_page(p);
	struct page *last = virt_to_page(p + size - 1);

	for (; page <= last; page++)
		if (PageSlab(page))
			__folio_clear_slab(page_folio(page));
}

void lx_mm_mark_slab(const void *p, size_t size)
{
	struct page *page = virt_to_page(p);
	struct page *last = virt_to_page(p + size - 1);

	for (; page <= last; page++)
		if (!PageSlab(page))
			__folio_set_slab(page_folio(page));
}
