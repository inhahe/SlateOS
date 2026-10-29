// SPDX-License-Identifier: MIT
/*
 * The driver core's view from outside, as a driver process has it: none.
 *
 * Linux's own driver core runs in every driver process (drivers/base,
 * lib/kobject.c; linuxdrv/emul/manifest), and as it adds each device, driver,
 * bus and class it also shows it in sysfs -- directories, attribute files,
 * links -- and announces it to user space (uevents). Nothing in a driver
 * process reads sysfs or listens for uevents, and the process has no file
 * system to show them in, so here every such call succeeds and keeps
 * nothing: an object's sysfs node stays NULL, which is what Linux's own code
 * finds for an object not shown, and every kernfs call it makes with one is
 * safe with NULL. What a driver itself keeps in an attribute -- its own
 * state behind a show() function -- is the driver's, unaffected.
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

#include "slx.h"
#include "lx_emul.h"

int sysfs_create_dir_ns(struct kobject *kobj, const void *ns)
{
	return 0;
}

void sysfs_remove_dir(struct kobject *kobj)
{
}

int sysfs_rename_dir_ns(struct kobject *kobj, const char *new_name, const void *new_ns)
{
	return 0;
}

int sysfs_move_dir_ns(struct kobject *kobj, struct kobject *new_parent_kobj, const void *new_ns)
{
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

/* Every node is NULL here (above), and Linux's own kernfs is safe with one. */
void kernfs_get(struct kernfs_node *kn)
{
}

void kernfs_put(struct kernfs_node *kn)
{
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

int lx_sysfs_init(void)
{
	kernel_kobj = kobject_create_and_add("kernel", NULL);
	return kernel_kobj ? 0 : -ENOMEM;
}

/* No device here is a disk or a partition: these only ever compare unequal. */
const struct class block_class = { .name = "block" };
const struct device_type part_type = { .name = "partition" };
