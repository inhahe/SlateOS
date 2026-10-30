// SPDX-License-Identifier: MIT
/*
 * The file systems Linux shows itself through, as a driver process has
 * them: none -- sysfs, procfs, and any mount. And the one file system it
 * reads: the system's own, through which its firmware loader reads a
 * driver's firmware (below).
 *
 * Linux's own driver core runs in every driver process (drivers/base,
 * lib/kobject.c; linuxdrv/emul/manifest), and as it adds each device, driver,
 * bus and class it also shows it in sysfs -- directories, attribute files,
 * links -- and announces it to user space (uevents). Nothing in a driver
 * process reads sysfs or listens for uevents, and the process has no file
 * system to show them in, so here every such call succeeds and keeps
 * nothing -- but a kobject's directory, which keeps what Linux's own code
 * reads of it (below). What a driver itself keeps in an attribute -- its
 * own state behind a show() function -- is the driver's, unaffected.
 *
 * procfs the same: Linux's own code shows its resource tree and its PCI
 * devices there (kernel/resource.c, drivers/pci/proc.c), and every entry it
 * makes is answered NULL -- no entry, which its code takes as procfs being
 * absent or full, and goes on without.
 *
 * Left out, and so generated stops: what only a reader or writer of sysfs
 * reaches -- an attribute removing itself from its own store()
 * (sysfs_remove_file_self), a file's owner changed for a network namespace
 * (sysfs_*change_owner) -- and the uevent variables only a uevent's own
 * callbacks add (add_uevent_var), which kobject_uevent_env() here never runs.
 *
 * And the identities the driver core compares a device against for block
 * devices, which a driver process has none of.
 */
#include <linux/sysfs.h>
#include <linux/kernfs.h>
#include <linux/kobject.h>
#include <linux/device.h>
#include <linux/blkdev.h>
#include <linux/fs.h>
#include <linux/user_namespace.h>
#include <linux/pid_namespace.h>
#include <linux/threads.h>
#include <linux/proc_fs.h>
#include <linux/kernel_read_file.h>
#include <linux/vmalloc.h>
#include <linux/slab.h>

#include "slx.h"
#include "lx_emul.h"

/*
 * A kobject's directory: a kernfs node shown nowhere and linked into no
 * tree, kept because Linux's own code reaches into it -- a namespaced
 * class, as the net class is, marks its directory so (lib/kobject.c's
 * create_dir() -> kernfs_enable_ns(), inline, which reads and writes the
 * node). Counted as Linux's kernfs counts one: a reference while the
 * kobject is shown, one more for each kernfs_get(); freed with the last.
 * Nothing reads its place in a tree -- a parent, the directories under it
 * (drivers/base/core.c's kobject_has_children() guards a race between two
 * CPUs, and the glue directory it asks about is kept by its references
 * here) -- so it keeps none.
 */
void kernfs_get(struct kernfs_node *kn)
{
	if (kn)
		atomic_inc(&kn->count);
}

void kernfs_put(struct kernfs_node *kn)
{
	if (kn && atomic_dec_and_test(&kn->count))
		kfree(kn);
}

int sysfs_create_dir_ns(struct kobject *kobj, const void *ns)
{
	struct kernfs_node *kn = kzalloc(sizeof(*kn), GFP_KERNEL);

	if (!kn)
		return -ENOMEM;
	atomic_set(&kn->count, 1);
	kn->flags = KERNFS_DIR;
	kn->ns = ns;
	kn->dir.children = RB_ROOT;
	kobj->sd = kn;
	return 0;
}

void sysfs_remove_dir(struct kobject *kobj)
{
	struct kernfs_node *kn = kobj->sd;

	kobj->sd = NULL;
	kernfs_put(kn);
}

int sysfs_rename_dir_ns(struct kobject *kobj, const char *new_name, const void *new_ns)
{
	if (kobj->sd)
		kobj->sd->ns = new_ns;
	return 0;
}

int sysfs_move_dir_ns(struct kobject *kobj, struct kobject *new_parent_kobj, const void *new_ns)
{
	if (kobj->sd)
		kobj->sd->ns = new_ns;
	return 0;
}

int sysfs_create_file_ns(struct kobject *kobj, const struct attribute *attr, const void *ns)
{
	return 0;
}

void sysfs_remove_file_ns(struct kobject *kobj, const struct attribute *attr, const void *ns)
{
}

int sysfs_create_bin_file(struct kobject *kobj, const struct bin_attribute *attr)
{
	return 0;
}

void sysfs_remove_bin_file(struct kobject *kobj, const struct bin_attribute *attr)
{
}

int sysfs_create_group(struct kobject *kobj, const struct attribute_group *grp)
{
	return 0;
}

int sysfs_create_groups(struct kobject *kobj, const struct attribute_group **groups)
{
	return 0;
}

int sysfs_update_group(struct kobject *kobj, const struct attribute_group *grp)
{
	return 0;
}

void sysfs_remove_group(struct kobject *kobj, const struct attribute_group *grp)
{
}

void sysfs_remove_groups(struct kobject *kobj, const struct attribute_group **groups)
{
}

int sysfs_create_link(struct kobject *kobj, struct kobject *target, const char *name)
{
	return 0;
}

void sysfs_remove_link(struct kobject *kobj, const char *name)
{
}

void sysfs_delete_link(struct kobject *dir, struct kobject *targ, const char *name)
{
}

int sysfs_rename_link_ns(struct kobject *kobj, struct kobject *target, const char *old_name,
			 const char *new_name, const void *new_ns)
{
	return 0;
}

/* Nobody listens: an event announced is an event delivered to no one. */
int kobject_uevent(struct kobject *kobj, enum kobject_action action)
{
	return 0;
}

int kobject_uevent_env(struct kobject *kobj, enum kobject_action action, char *envp[])
{
	return 0;
}

/*
 * The kernel's own object (/sys/kernel), under which subsystems put theirs
 * -- the software nodes' set among them -- made as Linux's ksysfs_init()
 * makes it, before the first initcall (lx_emul_main).
 */
struct kobject *kernel_kobj;

/*
 * Path names' cache, from which __getname() takes PATH_MAX bytes at a time
 * (the firmware loader builds each path it tries in one), made as Linux's
 * vfs_caches_init() makes it, before the first initcall.
 */
struct kmem_cache *names_cachep;

int lx_fs_init(void)
{
	kernel_kobj = kobject_create_and_add("kernel", NULL);
	names_cachep = kmem_cache_create("names_cache", PATH_MAX, 0, SLAB_HWCACHE_ALIGN, NULL);
	return kernel_kobj && names_cachep ? 0 : -ENOMEM;
}

/*
 * The open file `handle` read as Linux's kernel_read_file() reads one
 * (fs/kernel_read_file.c), its every check kept: from byte `offset` into
 * `*buf`, `buf_size` bytes at most -- a buffer of the file's size
 * vmalloc'ed there when `*buf` is NULL, and freed again on a failure --
 * and the file's size told at `*file_size` when asked. A read from 0
 * without `file_size` is of the whole file, which must fit and must all
 * be read. Bytes read, or a negative errno.
 *
 * Linux keeps writers out while it reads, and refuses a file a writer
 * holds (deny_write_access(), -ETXTBSY). The system has no such lock, so
 * a file written while it is read -- firmware rewritten in place by an
 * update -- is caught after the read: its change time, which every write
 * moves, is read again, and the read refused as Linux refuses one,
 * -ETXTBSY, if it moved. A write in the same tick of the system's clock as
 * the file's last change before the read is not seen.
 */
static ssize_t lx_read_file(long handle, loff_t offset, void **buf, size_t buf_size,
			    size_t *file_size)
{
	unsigned long long changed, changed_after;
	void *allocated = NULL;
	long long i_size, size_after;
	bool regular, whole_file;
	ssize_t copied = 0;
	loff_t pos = offset;
	int ret = 0;

	if (offset != 0 && (!*buf || !file_size))
		return -EINVAL;
	/* A size past SSIZE_MAX the kit answers -EFBIG, as Linux's check. */
	i_size = slx_file_stat(handle, &regular, &changed);
	if (i_size < 0)
		return i_size;
	if (!regular || i_size == 0)
		return -EINVAL;
	/* The entire file cannot be read in one buffer. */
	if (!file_size && offset == 0 && i_size > buf_size)
		return -EFBIG;
	whole_file = offset == 0 && i_size <= buf_size;
	if (file_size)
		*file_size = i_size;
	if (!*buf)
		*buf = allocated = vmalloc(i_size);
	if (!*buf)
		return -ENOMEM;
	while (copied < buf_size) {
		size_t wanted = min_t(size_t, buf_size - copied, i_size - pos);
		long bytes;

		if (!wanted)
			break;
		bytes = slx_file_read(handle, *buf + copied, wanted, pos);
		if (bytes < 0) {
			ret = bytes;
			goto out_free;
		}
		if (bytes == 0)
			break;
		pos += bytes;
		copied += bytes;
	}
	size_after = slx_file_stat(handle, &regular, &changed_after);
	if (size_after < 0)
		ret = size_after;
	else if (changed_after != changed)
		ret = -ETXTBSY;
	/* The file shrank while it was read. */
	else if (whole_file && pos != i_size)
		ret = -EIO;
out_free:
	if (ret < 0 && allocated) {
		vfree(allocated);
		*buf = NULL;
	}
	return ret ? ret : copied;
}

/*
 * The file at `path` read as lx_read_file() reads one: Linux reads it in
 * init's mount namespace, so that a firmware file is found whoever asked;
 * a driver process sees the system's one file system, with its own
 * authority -- what the process was given to read with (a File
 * capability), as whom it runs.
 */
ssize_t kernel_read_file_from_path_initns(const char *path, loff_t offset, void **buf,
					  size_t buf_size, size_t *file_size,
					  enum kernel_read_file_id id)
{
	ssize_t ret;
	long handle;

	if (!path || !*path)
		return -EINVAL;
	handle = slx_file_open(path);
	if (handle < 0)
		return handle;
	ret = lx_read_file(handle, offset, buf, buf_size, file_size);
	slx_file_close(handle);
	return ret;
}

struct proc_dir_entry *proc_mkdir(const char *name, struct proc_dir_entry *parent)
{
	return NULL;
}

struct proc_dir_entry *proc_create_data(const char *name, umode_t mode,
					struct proc_dir_entry *parent,
					const struct proc_ops *proc_ops, void *data)
{
	return NULL;
}

struct proc_dir_entry *proc_create_seq_private(const char *name, umode_t mode,
					       struct proc_dir_entry *parent,
					       const struct seq_operations *ops,
					       unsigned int state_size, void *data)
{
	return NULL;
}

void proc_set_size(struct proc_dir_entry *de, loff_t size)
{
}

void proc_remove(struct proc_dir_entry *de)
{
}

/*
 * No file system can be mounted in a driver process: the one Linux's own
 * core tries is kernel/resource.c's iomem file system, through which it
 * revokes /dev/mem mappings of a region a driver claims, and there is no
 * /dev/mem here to revoke. Its fs_initcall is told so, and carried past;
 * the file system's type names kill_anon_super, never reached.
 */
int simple_pin_fs(struct file_system_type *type, struct vfsmount **mount, int *count)
{
	return -ENODEV;
}

void kill_anon_super(struct super_block *sb)
{
	slx_stop("kill_anon_super", __builtin_return_address(0));
}

/*
 * The one user namespace, which only procfs's view of the resource tree
 * asks after (/proc/iomem), and a driver process has no procfs.
 */
struct user_namespace init_user_ns;

/*
 * The one process-id namespace, which the namespace code asks after when a
 * task wants to see every namespace (kernel/nscommon.c); a driver process's
 * tasks have no process ids.
 */
struct pid_namespace init_pid_ns = {
	.ns.__ns_ref = REFCOUNT_INIT(2),
	.idr = IDR_INIT(init_pid_ns.idr),
	.level = 0,
	.user_ns = &init_user_ns,
	.pid_max = PID_MAX_DEFAULT,
};

/* No device here is a disk or a partition: these only ever compare unequal. */
const struct class block_class = { .name = "block" };
const struct device_type part_type = { .name = "partition" };
