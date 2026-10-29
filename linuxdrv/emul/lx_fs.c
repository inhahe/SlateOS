// SPDX-License-Identifier: MIT
/*
 * The file systems Linux shows itself through, as a driver process has
 * them: none -- sysfs, procfs, and any mount.
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

int lx_fs_init(void)
{
	kernel_kobj = kobject_create_and_add("kernel", NULL);
	return kernel_kobj ? 0 : -ENOMEM;
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
