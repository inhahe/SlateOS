// SPDX-License-Identifier: MIT
/*
 * The PCI functions a driver process was given, as Linux's own PCI core
 * sees them (drivers/pci, compiled in; design-decisions 1584).
 *
 * The process is started with an argument `pci=BB:DD.F` (bus, device and
 * function, in hex) for each function it may drive, and a capability for
 * each. Where Linux on x86 scans its buses -- at subsystem level, once the
 * PCI bus type is registered -- the layer opens each function's device
 * handle (design-decisions 1579) and scans a root bus of its number whose
 * configuration accessors (lx_pci_ops) are the handle's: Linux's own
 * probe.c then reads the function as it reads any -- ids, class,
 * capabilities, BARs -- claims its BARs in the resource tree, and the driver
 * core matches it with the driver's pci_driver when that registers. A
 * function on the bus that the process was not given reads as absent.
 *
 * The handle refuses writes to the registers the kernel manages: the BARs,
 * the expansion ROM, the MSI and MSI-X capabilities, the power state, a
 * function reset. The BARs and the ROM the system-facing half answers as
 * the hardware would after what Linux wrote (linuxdrv/kit/src/bars.rs), so
 * that Linux's probe sizes them as it does on hardware while the device
 * stays where the kernel put it; a BAR Linux moved is refused when it is
 * mapped (ioremap). Any other refused write fails as the hardware's own
 * failure would (PCIBIOS_SET_FAILED), and Linux's own code goes on.
 *
 * After the initcalls, a function no driver took ends the process
 * (lx_pci_check_bound): a driver process exists to drive what it was given.
 */
#include <linux/pci.h>
#include <linux/ioport.h>
#include <linux/io.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/msi.h>
#include <linux/irqdomain.h>

#include "slx.h"
#include "lx_emul.h"

/* As many as the system-facing half keeps (linuxdrv/kit/src/pci.rs). */
#define LX_PCI_FUNCTIONS LX_PCI_FUNCTIONS_MAX

/* A function given: its handle, where it is, its BARs' mappings once made. */
struct lx_pci_function {
	long handle;
	unsigned int bus, devfn;
	void __iomem *mapped[PCI_STD_NUM_BARS];
	struct pci_dev *dev;
};

/*
 * What x86's own code would set, which Linux's PCI quirks and resource
 * assignment read: no IO-APIC quirks or rerouting, the kernel owning
 * interrupts; no
 * IO-APIC known; and where PCI memory would be assigned from -- nothing is
 * assigned here, every BAR being where the kernel found it.
 */
int noioapicquirk = 1;
int noioapicreroute = 1;
int nr_ioapics;
unsigned long pci_mem_start;

static struct lx_pci_function lx_pci_functions[LX_PCI_FUNCTIONS];
static unsigned int lx_pci_count;

/*
 * x86's own hooks into the PCI core, as the process has its devices: a
 * device's legacy interrupt line is not routed here -- its interrupts are
 * the MSI the kernel programs through its handle -- and bus numbers are
 * never reassigned, every bus being where the kernel found it.
 */
int pcibios_alloc_irq(struct pci_dev *dev)
{
	return 0;
}

void pcibios_free_irq(struct pci_dev *dev)
{
}

unsigned int pcibios_assign_all_busses(void)
{
	return 0;
}

/*
 * No interrupt domain for MSI: Linux would program a device's MSI through
 * one, where here the kernel programs it through the handle. A device
 * found on a bus is given none.
 */
struct irq_domain *pci_msi_get_device_domain(struct pci_dev *pdev)
{
	return NULL;
}

struct irq_domain *irq_find_matching_fwspec(struct irq_fwspec *fwspec,
					    enum irq_domain_bus_token bus_token)
{
	return NULL;
}

static struct lx_pci_function *lx_pci_find(unsigned int bus, unsigned int devfn)
{
	unsigned int i;

	for (i = 0; i < lx_pci_count; i++)
		if (lx_pci_functions[i].bus == bus && lx_pci_functions[i].devfn == devfn)
			return &lx_pci_functions[i];
	return NULL;
}

static int lx_pci_read(struct pci_bus *bus, unsigned int devfn, int where, int size, u32 *val)
{
	struct lx_pci_function *f = lx_pci_find(bus->number, devfn);
	long got;

	if (!f) {
		*val = ~0U;
		return PCIBIOS_DEVICE_NOT_FOUND;
	}
	got = slx_pci_config_read(f->handle, where, size);
	if (got < 0) {
		*val = ~0U;
		return PCIBIOS_BAD_REGISTER_NUMBER;
	}
	*val = got;
	return PCIBIOS_SUCCESSFUL;
}

static int lx_pci_write(struct pci_bus *bus, unsigned int devfn, int where, int size, u32 val)
{
	struct lx_pci_function *f = lx_pci_find(bus->number, devfn);

	if (!f)
		return PCIBIOS_DEVICE_NOT_FOUND;
	return slx_pci_config_write(f->handle, where, size, val) < 0 ? PCIBIOS_SET_FAILED :
								    PCIBIOS_SUCCESSFUL;
}

static struct pci_ops lx_pci_ops = {
	.read = lx_pci_read,
	.write = lx_pci_write,
};

/* One `pci=BB:DD.F` argument taken: the function opened. 0, or an error. */
static int lx_pci_take(const char *arg)
{
	unsigned int bus, dev, fn;
	struct lx_pci_function *f;
	int used = 0;

	if (sscanf(arg, "pci=%x:%x.%x%n", &bus, &dev, &fn, &used) != 3 || arg[used] ||
	    bus > 0xff || dev > 0x1f || fn > 7) {
		pr_err("pci: '%s' names no function (pci=BB:DD.F, in hex)\n", arg);
		return -EINVAL;
	}
	if (lx_pci_count == LX_PCI_FUNCTIONS) {
		pr_err("pci: more than %d functions given\n", LX_PCI_FUNCTIONS);
		return -E2BIG;
	}
	f = &lx_pci_functions[lx_pci_count];
	f->bus = bus;
	f->devfn = PCI_DEVFN(dev, fn);
	if (lx_pci_find(f->bus, f->devfn)) {
		pr_err("pci: %02x:%02x.%x given twice\n", bus, dev, fn);
		return -EEXIST;
	}
	f->handle = slx_pci_open(f->bus, f->devfn);
	if (f->handle < 0) {
		pr_err("pci: %02x:%02x.%x could not be opened: %ld\n", bus, dev, fn, f->handle);
		return -ENODEV;
	}
	lx_pci_count++;
	return 0;
}

/*
 * Each bus a given function is on, made as Linux's x86 code makes a root
 * bus -- the memory space and the one bus number its windows, its sysdata
 * the x86 kind (domain 0, no NUMA node) -- with only the functions given
 * scanned on it: a slot's other functions are not the process's, and
 * Linux's own slot scan would stop at a function 0 it cannot read (the
 * I219 e1000e drives on a PCH is 00:1f.6).
 */
static int lx_pci_scan_bus(unsigned int number)
{
	struct pci_sysdata *sysdata = kzalloc(sizeof(*sysdata), GFP_KERNEL);
	struct resource *busn = kzalloc(sizeof(*busn), GFP_KERNEL);
	struct pci_bus *bus;
	LIST_HEAD(resources);
	unsigned int i;

	if (!sysdata || !busn)
		return -ENOMEM;
	sysdata->node = NUMA_NO_NODE;
	busn->name = "PCI busn";
	busn->start = busn->end = number;
	busn->flags = IORESOURCE_BUS;
	pci_add_resource(&resources, &iomem_resource);
	pci_add_resource(&resources, busn);
	bus = pci_create_root_bus(NULL, number, &lx_pci_ops, sysdata, &resources);
	if (!bus) {
		pci_free_resource_list(&resources);
		return -ENODEV;
	}
	for (i = 0; i < lx_pci_count; i++) {
		struct lx_pci_function *f = &lx_pci_functions[i];

		if (f->bus == number)
			f->dev = pci_dev_get(pci_scan_single_device(bus, f->devfn));
	}
	pci_bus_claim_resources(bus);
	pci_bus_add_devices(bus);
	return 0;
}

/*
 * The scan, at subsystem level as x86 Linux's own (pci_subsys_init): after
 * the PCI bus type and class (postcore), before any driver registers. A
 * function given that cannot be opened or found ends the process: its
 * driver would have nothing to drive.
 */
static int __init lx_pci_scan(void)
{
	unsigned int i, j;
	int err = 0;

	/* The two halves' tables must agree on how many functions there may be. */
	if (slx_pci_functions() != LX_PCI_FUNCTIONS) {
		pr_err("pci: the layer's halves disagree on how many functions a process may have (%lu, %d)\n",
		       slx_pci_functions(), LX_PCI_FUNCTIONS);
		slx_exit(SLX_EXIT_BUG);
	}
	for (i = 1; i < slx_argc() && !err; i++)
		if (!strncmp(slx_argv(i), "pci=", 4))
			err = lx_pci_take(slx_argv(i));
	for (i = 0; i < lx_pci_count && !err; i++) {
		for (j = 0; j < i; j++)
			if (lx_pci_functions[j].bus == lx_pci_functions[i].bus)
				break;
		if (j == i)
			err = lx_pci_scan_bus(lx_pci_functions[i].bus);
	}
	for (i = 0; i < lx_pci_count && !err; i++)
		if (!lx_pci_functions[i].dev) {
			pr_err("pci: %02x:%02x.%x was opened but not found on its bus\n",
			       lx_pci_functions[i].bus, PCI_SLOT(lx_pci_functions[i].devfn),
			       PCI_FUNC(lx_pci_functions[i].devfn));
			err = -ENODEV;
		}
	if (err)
		slx_exit(SLX_EXIT_INIT_FAILED);
	return 0;
}
subsys_initcall(lx_pci_scan);

int lx_pci_index(const struct pci_dev *dev)
{
	unsigned int i;

	for (i = 0; i < lx_pci_count; i++)
		if (lx_pci_functions[i].dev == dev)
			return i;
	return -1;
}

long lx_pci_handle(int i)
{
	return i >= 0 && i < lx_pci_count ? lx_pci_functions[i].handle : -1;
}

bool lx_pci_check_bound(void)
{
	bool bound = true;
	unsigned int i;

	wait_for_device_probe();
	for (i = 0; i < lx_pci_count; i++)
		if (!lx_pci_functions[i].dev->dev.driver) {
			pci_err(lx_pci_functions[i].dev, "no driver took it\n");
			bound = false;
		}
	return bound;
}

/*
 * A BAR's registers mapped, where Linux's x86 ioremap() would map them: the
 * memory BAR of a given function that holds [phys, phys + size), mapped
 * whole (once, uncached) through its handle. Anything else -- memory that
 * is no BAR of the process's, a BAR Linux moved -- is refused, which a
 * driver reads as no memory to map.
 */
static void __iomem *lx_ioremap(resource_size_t phys, unsigned long size)
{
	unsigned int i, b;

	for (i = 0; i < lx_pci_count; i++) {
		struct lx_pci_function *f = &lx_pci_functions[i];

		for (b = 0; b < PCI_STD_NUM_BARS; b++) {
			u64 base, len, seen;
			int at = slx_pci_bar_window(f->handle, b, &base, &len, &seen);
			long mapped;

			if (!at)
				continue;
			/* Where Linux moved it to: the device is not there. */
			if (at < 0 && phys >= seen && phys - seen < len) {
				pci_err(f->dev, "ioremap of %#llx: BAR %u was moved there from %#llx, which the device cannot follow\n",
					(unsigned long long)phys, b, base);
				return NULL;
			}
			if (phys < base || phys - base >= len || size > len - (phys - base))
				continue;
			if (!f->mapped[b]) {
				mapped = slx_pci_map_bar(f->handle, b);
				if (mapped <= 0) {
					/* NotSupported (-2): the handle maps whole BARs of a page or more. */
					pci_err(f->dev, "BAR %u could not be mapped: %ld%s\n", b, mapped,
						mapped == -2 ? " (a BAR under 4 KiB, or not page-aligned)" : "");
					return NULL;
				}
				f->mapped[b] = (void __iomem *)mapped;
			}
			return f->mapped[b] + (phys - base);
		}
	}
	pr_err("ioremap of %#llx (%lu bytes): no BAR of the process's holds it\n",
	       (unsigned long long)phys, size);
	return NULL;
}

void __iomem *ioremap(resource_size_t offset, unsigned long size)
{
	return lx_ioremap(offset, size);
}

void __iomem *ioremap_uc(resource_size_t offset, unsigned long size)
{
	return lx_ioremap(offset, size);
}

/*
 * Write-combining asked for, uncached given: the handle maps a BAR so, and
 * combining writes is a speed a device never needs for correctness. So x86's
 * page attribute table, which would give it, reads as off to Linux's code.
 */
bool pat_enabled(void)
{
	return false;
}

void __iomem *ioremap_wc(resource_size_t offset, unsigned long size)
{
	return lx_ioremap(offset, size);
}

/* A BAR stays mapped until the handle closes, at the process's exit. */
void iounmap(volatile void __iomem *addr)
{
}
