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
#include <linux/gfp.h>
#include <linux/slab.h>
#include <linux/vmalloc.h>
#include <linux/page_ref.h>

/* The page allocator's own helpers for compound pages. */
#include <../mm/internal.h>

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

/*
 * The page allocator, as a driver process has it: a block of 2^order pages
 * from kmalloc's allocator (whose blocks are aligned to their size, so to a
 * page at least), its pages' entries set up as Linux's are -- the first's
 * count 1, and a compound page's head and tails marked (__GFP_COMP) with
 * the page allocator's own helpers (mm/internal.h) -- and cleared again when
 * it is freed, so that whatever is handed out there next starts from
 * nothing. Page fragments (mm/page_frag_cache.c, Linux's own), which socket
 * buffers are cut from, stand on it.
 */
struct page *__alloc_pages_noprof(gfp_t gfp, unsigned int order, int preferred_nid,
				  nodemask_t *nodemask)
{
	unsigned long i, pages = 1UL << order;
	struct page *page;
	void *p;

	if (order > MAX_PAGE_ORDER)
		return NULL;
	p = slx_alloc(PAGE_SIZE << order);
	if (!p)
		return NULL;
	page = virt_to_page(p);
	memset(page, 0, sizeof(*page) * pages);
	set_page_count(page, 1);
	if (order && (gfp & __GFP_COMP)) {
		__SetPageHead(page);
		for (i = 1; i < pages; i++)
			prep_compound_tail(page, i);
		prep_compound_head(page, order);
	}
	if (gfp & __GFP_ZERO)
		memset(p, 0, PAGE_SIZE << order);
	return page;
}

static void lx_free_pages(struct page *page, unsigned int order)
{
	void *p = page_address(page);

	memset(page, 0, sizeof(*page) << order);
	slx_free(p);
}

/*
 * Linux frees a block once its first page's count drops to 0. A block not
 * compound whose first page another holder still counts would have its
 * other pages freed and the first kept; a block from kmalloc's allocator
 * cannot be split so, and is kept whole, said once.
 */
void __free_pages(struct page *page, unsigned int order)
{
	if (put_page_testzero(page))
		lx_free_pages(page, order);
	else
		WARN_ONCE(order && !PageHead(page),
			  "__free_pages: an order-%u block still counted elsewhere is kept whole\n",
			  order);
}

void free_pages(unsigned long addr, unsigned int order)
{
	if (addr)
		__free_pages(virt_to_page((void *)addr), order);
}

unsigned long get_free_pages_noprof(gfp_t gfp, unsigned int order)
{
	struct page *page = __alloc_pages_noprof(gfp & ~__GFP_HIGHMEM, order, numa_node_id(), NULL);

	return page ? (unsigned long)page_address(page) : 0;
}

/* A folio whose last reference went (folio_put): freed whole. */
void __folio_put(struct folio *folio)
{
	lx_free_pages(&folio->page, folio_order(folio));
}

/*
 * vmalloc: memory contiguous where the process sees it, which kmalloc's
 * already is -- the heap is one range -- so it comes from there, a page at
 * least and page-aligned as Linux's, its pages no slab's. It is direct-map
 * memory here, so is_vmalloc_addr() answers no for it, and vfree() and
 * kvfree() give it back as kfree() would.
 */
static void *lx_vmalloc(unsigned long size, gfp_t gfp)
{
	unsigned long bytes = max(size, PAGE_SIZE);
	void *p;

	if (!size)
		return NULL;
	p = slx_alloc(bytes);
	if (!p)
		return NULL;
	lx_mm_mark_pages(p, bytes);
	if (gfp & __GFP_ZERO)
		memset(p, 0, bytes);
	return p;
}

void *__vmalloc_noprof(unsigned long size, gfp_t gfp)
{
	return lx_vmalloc(size, gfp);
}

void *vmalloc_noprof(unsigned long size)
{
	return lx_vmalloc(size, GFP_KERNEL);
}

void *vzalloc_noprof(unsigned long size)
{
	return lx_vmalloc(size, GFP_KERNEL | __GFP_ZERO);
}

void vfree(const void *addr)
{
	kfree(addr);
}

/* kvmalloc: kmalloc's, whatever the size, aligned to `align` at least. */
void *__kvmalloc_node_noprof(DECL_BUCKET_PARAMS(size, b), unsigned long align, gfp_t flags,
			     int node)
{
	return kmalloc(max_t(size_t, size, align), flags);
}

/*
 * A block grown or shrunk, its contents kept to the smaller size: in place
 * while the block holds `new_size` -- the bytes past it zeroed when
 * __GFP_ZERO asks, so that a later growth finds them so -- else moved to a
 * new block, the old one freed. On failure the old block is untouched.
 */
void *krealloc_node_align_noprof(const void *p, size_t new_size, unsigned long align, gfp_t flags,
				 int nid)
{
	size_t held = ZERO_OR_NULL_PTR(p) ? 0 : ksize(p);
	void *n;

	if (!new_size) {
		kfree(p);
		return ZERO_SIZE_PTR;
	}
	if (held >= new_size && IS_ALIGNED((unsigned long)p, align)) {
		if (flags & __GFP_ZERO)
			memset((void *)p + new_size, 0, held - new_size);
		return (void *)p;
	}
	n = kmalloc(max_t(size_t, new_size, align), flags);
	if (!n)
		return NULL;
	if (held)
		memcpy(n, p, min(held, new_size));
	kfree(p);
	return n;
}

void *kvrealloc_node_align_noprof(const void *p, size_t size, unsigned long align, gfp_t flags,
				  int nid)
{
	return krealloc_node_align_noprof(p, size, align, flags, nid);
}
