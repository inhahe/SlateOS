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
 * the IOMMU refused would (DMA_MAPPING_ERROR), said once. So does any DMA
 * for a device other than the window's, the first function given: its own
 * IOMMU domain holds none of the window's arenas.
 *
 * x86's DMA is cache-coherent, and the arenas are mapped write-back as the
 * kernel's view of them is, so there is nothing to sync and no bounce
 * buffer: a device is marked as needing no syncs (dma_skip_sync), and the
 * syncs Linux still calls do nothing.
 */
#include <linux/dma-mapping.h>
#include <linux/device.h>
#include <linux/pci.h>
#include <linux/mm.h>
#include <linux/slab.h>
#include <linux/string.h>

#include "slx.h"
#include "lx_emul.h"

/*
 * What a mapping of `size` bytes at `virt` answers: its device address, if
 * `dev` can be given it -- `dev` the window's device, the bytes inside one
 * chunk it backed, and the last of them within `mask`.
 */
static dma_addr_t lx_dma_address(struct device *dev, const void *virt, size_t size, u64 mask)
{
	u64 addr = lx_pci_dma_device(dev) ? slx_dma_address(virt, size) : U64_MAX;

	if (addr == U64_MAX || !size || addr + (size - 1) > mask) {
		dev_err_once(dev, "DMA of %zu bytes at %px: not memory the device reaches (mask %#llx)\n",
			     size, virt, mask);
		return DMA_MAPPING_ERROR;
	}
	return addr;
}

/* The streaming mask: what a mapping must lie within. */
static u64 lx_dma_mask(struct device *dev)
{
	return dev->dma_mask ? *dev->dma_mask : 0;
}

dma_addr_t dma_map_page_attrs(struct device *dev, struct page *page, size_t offset,
			      size_t size, enum dma_data_direction dir, unsigned long attrs)
{
	return lx_dma_address(dev, page_address(page) + offset, size, lx_dma_mask(dev));
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
 * Coherent memory: from the window alone, within the coherent mask, aligned
 * to its size rounded up to a power of two (the window's blocks are), as
 * Linux's own is at least page-aligned. The whole block is zeroed and said
 * to be no slab's, as Linux zeroes the whole page-rounded allocation: a
 * driver may hand its device the page.
 */
void *dma_alloc_attrs(struct device *dev, size_t size, dma_addr_t *dma_handle, gfp_t flag,
		      unsigned long attrs)
{
	void *p = slx_dma_alloc(max_t(size_t, size, PAGE_SIZE));
	unsigned long bytes;

	if (!p)
		return NULL;
	*dma_handle = lx_dma_address(dev, p, size, dev->coherent_dma_mask);
	if (*dma_handle == DMA_MAPPING_ERROR) {
		slx_free(p);
		return NULL;
	}
	bytes = slx_block_size(p);
	lx_mm_mark_pages(p, bytes);
	memset(p, 0, bytes);
	return p;
}

void dma_free_attrs(struct device *dev, size_t size, void *cpu_addr, dma_addr_t dma_handle,
		    unsigned long attrs)
{
	slx_free(cpu_addr);
}

/* What dmam_alloc_attrs() hands back when the device goes. */
struct lx_dmam {
	void *cpu_addr;
	dma_addr_t dma_handle;
	size_t size;
	unsigned long attrs;
};

static void lx_dmam_release(struct device *dev, void *res)
{
	struct lx_dmam *m = res;

	dma_free_attrs(dev, m->size, m->cpu_addr, m->dma_handle, m->attrs);
}

/* Coherent memory as dma_alloc_attrs() gives it, freed when the device is released (devres). */
void *dmam_alloc_attrs(struct device *dev, size_t size, dma_addr_t *dma_handle, gfp_t gfp,
		       unsigned long attrs)
{
	struct lx_dmam *m = devres_alloc(lx_dmam_release, sizeof(*m), gfp);
	void *p;

	if (!m)
		return NULL;
	p = dma_alloc_attrs(dev, size, dma_handle, gfp, attrs);
	if (!p) {
		devres_free(m);
		return NULL;
	}
	*m = (struct lx_dmam){ .cpu_addr = p, .dma_handle = *dma_handle, .size = size, .attrs = attrs };
	devres_add(dev, m);
	return p;
}

/*
 * The widths a device's masks give its arenas. The window serves both
 * streaming mappings and coherent memory, so the chunks to come are kept
 * within the narrower of the two masks, at the widest of the kernel's
 * widths (64 -- any --, 32, 24) inside it. A width that a chunk already
 * backed exceeds is refused, as Linux refuses a mask its platform cannot
 * meet: kmalloc already serves from that chunk, and what it hands out would
 * be memory the device cannot reach. Only the window's device has DMA.
 */
static int lx_dma_set_width(struct device *dev, u64 streaming, u64 coherent)
{
	u64 mask = min(streaming, coherent);

	if (!lx_pci_dma_device(dev)) {
		dev_err(dev, "DMA: only the first function given has a DMA window\n");
		return -EIO;
	}
	if (mask < DMA_BIT_MASK(24))
		return -EIO;
	if (slx_dma_bits(mask == DMA_BIT_MASK(64) ? 64 : mask >= DMA_BIT_MASK(32) ? 32 : 24) < 0) {
		dev_err(dev, "DMA mask %#llx: memory already given to the device lies beyond it\n",
			mask);
		return -EIO;
	}
	return 0;
}

int dma_set_mask(struct device *dev, u64 mask)
{
	int err;

	if (!dev->dma_mask)
		return -EIO;
	err = lx_dma_set_width(dev, mask, dev->coherent_dma_mask);
	if (!err)
		*dev->dma_mask = mask;
	return err;
}

int dma_set_coherent_mask(struct device *dev, u64 mask)
{
	int err = lx_dma_set_width(dev, lx_dma_mask(dev), mask);

	if (!err)
		dev->coherent_dma_mask = mask;
	return err;
}
