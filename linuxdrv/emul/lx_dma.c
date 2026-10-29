// SPDX-License-Identifier: MIT
/*
 * DMA in a driver process (design-decisions 1586): Linux's DMA API over
 * the process's DMA window.
 *
 * The window is a range beside the heap, inside Linux's direct map
 * (lx_mm.c), backed a chunk at a time by the device's own DMA memory -- the
 * kernel's arenas in its IOMMU domain (design-decisions 1579) -- as
 * allocations need it (linuxdrv/kit, window.rs). Once a function is given
 * to the process its window is opened, and every kmalloc() comes from it
 * while it can hold the block: so what a driver hands its device -- a
 * descriptor ring, a socket buffer -- is memory the device reaches, and a
 * DMA address is arithmetic, not a system call per packet. Memory outside
 * the window -- allocated before the device came, the process's own data,
 * a task's stack -- has no DMA address, and mapping it fails as a mapping
 * the IOMMU refused would (DMA_MAPPING_ERROR), said once.
 *
 * x86's DMA is cache-coherent, and the arenas are mapped write-through, so
 * there is nothing to sync and no bounce buffer: a device is marked as
 * needing no syncs (dma_skip_sync), and the syncs Linux still calls do
 * nothing.
 */
#include <linux/dma-mapping.h>
#include <linux/pci.h>
#include <linux/mm.h>
#include <linux/slab.h>
#include <linux/string.h>

#include "slx.h"
#include "lx_emul.h"

/* What a mapping of `size` bytes at `virt` answers: its device address, if the device can be given it. */
static dma_addr_t lx_dma_address(struct device *dev, const void *virt, size_t size)
{
	u64 addr = slx_dma_address(virt, size);
	u64 mask = dev->dma_mask ? *dev->dma_mask : 0;

	if (addr == U64_MAX || !size || addr + (size - 1) > mask) {
		dev_err_once(dev, "DMA of %zu bytes at %px: not memory the device reaches (mask %#llx)\n",
			     size, virt, mask);
		return DMA_MAPPING_ERROR;
	}
	return addr;
}

dma_addr_t dma_map_page_attrs(struct device *dev, struct page *page, size_t offset,
			      size_t size, enum dma_data_direction dir, unsigned long attrs)
{
	return lx_dma_address(dev, page_address(page) + offset, size);
}

/* A mapping holds nothing: its memory is the device's for as long as the window lives. */
void dma_unmap_page_attrs(struct device *dev, dma_addr_t addr, size_t size,
			  enum dma_data_direction dir, unsigned long attrs)
{
}

void __dma_sync_single_for_cpu(struct device *dev, dma_addr_t addr, size_t size,
			       enum dma_data_direction dir)
{
}

void __dma_sync_single_for_device(struct device *dev, dma_addr_t addr, size_t size,
				  enum dma_data_direction dir)
{
}

bool __dma_need_sync(struct device *dev, dma_addr_t dma_addr)
{
	return false;
}

/*
 * Coherent memory: zeroed, from the window alone, aligned to its size
 * rounded up to a power of two (the window's blocks are), as Linux's own is
 * at least page-aligned. Its pages are no slab's, as page-allocator memory
 * is not.
 */
void *dma_alloc_attrs(struct device *dev, size_t size, dma_addr_t *dma_handle, gfp_t flag,
		      unsigned long attrs)
{
	void *p = slx_dma_alloc(max_t(size_t, size, PAGE_SIZE));

	if (!p)
		return NULL;
	*dma_handle = lx_dma_address(dev, p, size);
	if (*dma_handle == DMA_MAPPING_ERROR) {
		slx_free(p);
		return NULL;
	}
	lx_mm_mark_pages(p, size);
	memset(p, 0, size);
	return p;
}

void dma_free_attrs(struct device *dev, size_t size, void *cpu_addr, dma_addr_t dma_handle,
		    unsigned long attrs)
{
	slx_free(cpu_addr);
}

/*
 * The addresses a device may be given. The window's chunks to come are
 * kept within the mask (slx_dma_bits); those backed before it are the
 * device's own, of whatever width it had then, and a mapping the new mask
 * refuses fails when it is asked for.
 */
int dma_set_mask(struct device *dev, u64 mask)
{
	if (!dev->dma_mask || mask < DMA_BIT_MASK(24))
		return -EIO;
	*dev->dma_mask = mask;
	/* The kernel's widths: 64 (any), 32, 24 -- the widest within the mask. */
	slx_dma_bits(mask == DMA_BIT_MASK(64) ? 64 : mask >= DMA_BIT_MASK(32) ? 32 : 24);
	return 0;
}

int dma_set_coherent_mask(struct device *dev, u64 mask)
{
	if (mask < DMA_BIT_MASK(24))
		return -EIO;
	dev->coherent_dma_mask = mask;
	return 0;
}
