//! `disk-utils/fdisk-menu.c`: the commands, which of them the current label
//! offers (in normal and in expert mode, in a nested table or not), the
//! help that lists them, and what each one does.

use libfdisk::errno::{ECANCELED, EINVAL};
use libfdisk::script::{Script, new_script_from_file};
use libfdisk::{Context, LabelType, Partition, fieldtype};

use crate::{BUFSIZ, Fdisk, get_user_reply, rpmatch};
use fdisk::list::{self, WIPEMODE_ALWAYS, WIPEMODE_AUTO};
use quoting::escape_unprintable;

/// `enum fdisk_labeltype` as the bits menus test.
fn bits(t: LabelType) -> u32 {
    match t {
        LabelType::Dos => 1 << 1,
        LabelType::Sun => 1 << 2,
        LabelType::Sgi => 1 << 3,
        LabelType::Bsd => 1 << 4,
        LabelType::Gpt => 1 << 5,
    }
}

const DOS: u32 = 1 << 1;
const SUN: u32 = 1 << 2;
const SGI: u32 = 1 << 3;
const BSD: u32 = 1 << 4;
const GPT: u32 = 1 << 5;

/// `struct menu_entry`.
pub struct MenuEntry {
    /// The command's key; `-` for a separator.
    key: u8,
    /// Its help line, or a separator's title.
    title: &'static str,
    /// Offered in normal mode.
    normal: bool,
    /// Offered in expert mode.
    expert: bool,
    /// Taken, but not listed in the help.
    hidden: bool,
    /// Only for these labels.
    label: u32,
    /// For every label but these.
    exclude: u32,
    /// Only in a table nested in one of these.
    parent: u32,
}

/// A menu entry's fields, the rest off: upstream's initialiser macros.
const fn entry(key: u8, title: &'static str, normal: bool, expert: bool) -> MenuEntry {
    MenuEntry {
        key,
        title,
        normal,
        expert,
        hidden: false,
        label: 0,
        exclude: 0,
        parent: 0,
    }
}

/// `MENU_SEP (t)`, `MENU_XSEP (t)` and `MENU_BSEP (t)`.
const fn sep(t: &'static str) -> MenuEntry {
    entry(b'-', t, true, false)
}
const fn xsep(t: &'static str) -> MenuEntry {
    entry(b'-', t, false, true)
}
const fn bsep(t: &'static str) -> MenuEntry {
    entry(b'-', t, true, true)
}
/// `MENU_ENT (k, t)`, and with `_E` an excluded label, `_L` the only one.
const fn ent(k: u8, t: &'static str) -> MenuEntry {
    entry(k, t, true, false)
}
const fn ent_e(k: u8, t: &'static str, l: u32) -> MenuEntry {
    MenuEntry {
        exclude: l,
        ..entry(k, t, true, false)
    }
}
const fn ent_l(k: u8, t: &'static str, l: u32) -> MenuEntry {
    MenuEntry {
        label: l,
        ..entry(k, t, true, false)
    }
}
/// `MENU_XENT (k, t)`, and `_H` hidden.
const fn xent(k: u8, t: &'static str) -> MenuEntry {
    entry(k, t, false, true)
}
const fn xent_h(k: u8, t: &'static str) -> MenuEntry {
    MenuEntry {
        hidden: true,
        ..entry(k, t, false, true)
    }
}
/// `MENU_BENT (k, t)`: both modes; `_E` excluding a label.
const fn bent(k: u8, t: &'static str) -> MenuEntry {
    entry(k, t, true, true)
}
const fn bent_e(k: u8, t: &'static str, l: u32) -> MenuEntry {
    MenuEntry {
        exclude: l,
        ..entry(k, t, true, true)
    }
}
/// `MENU_ENT_NEST (k, t, l, p)` and `MENU_BENT_NEST_H (k, t, l, p)`.
const fn ent_nest(k: u8, t: &'static str, l: u32, p: u32) -> MenuEntry {
    MenuEntry {
        label: l,
        parent: p,
        ..entry(k, t, true, false)
    }
}
const fn bent_nest_h(k: u8, t: &'static str, l: u32, p: u32) -> MenuEntry {
    MenuEntry {
        label: l,
        parent: p,
        hidden: true,
        ..entry(k, t, true, true)
    }
}

/// Which menu's callback does a command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Callback {
    Generic,
    CreateLabel,
    Geo,
    Gpt,
    Sun,
    Sgi,
    Dos,
    Bsd,
}

/// `struct menu`.
struct Menu {
    /// Only for these labels.
    label: u32,
    /// For every label but these.
    exclude: u32,
    /// Not in a nested table.
    nonested: bool,
    callback: Callback,
    entries: &'static [MenuEntry],
}

static MENU_GENERIC: Menu = Menu {
    label: 0,
    exclude: 0,
    nonested: false,
    callback: Callback::Generic,
    entries: &[
        bsep("Generic"),
        ent(b'd', "delete a partition"),
        ent(b'F', "list free unpartitioned space"),
        ent(b'l', "list known partition types"),
        ent(b'n', "add a new partition"),
        bent(b'p', "print the partition table"),
        ent(b't', "change a partition type"),
        bent_e(b'v', "verify the partition table", BSD),
        ent(b'i', "print information about a partition"),
        xent(
            b'd',
            "print the raw data of the first sector from the device",
        ),
        xent(b'D', "print the raw data of the disklabel from the device"),
        xent(b'f', "fix partitions order"),
        sep("Misc"),
        bent(b'm', "print this menu"),
        ent_e(b'u', "change display/entry units", GPT),
        ent_e(b'x', "extra functionality (experts only)", BSD),
        sep("Script"),
        ent(b'I', "load disk layout from sfdisk script file"),
        ent(b'O', "dump disk layout to sfdisk script file"),
        bsep("Save & Exit"),
        ent_e(b'w', "write table to disk and exit", BSD),
        ent_l(b'w', "write table to disk", BSD),
        bent(b'q', "quit without saving changes"),
        xent(b'r', "return to main menu"),
        ent_nest(b'r', "return from BSD to DOS (MBR)", BSD, DOS),
        ent_nest(b'r', "return from protective/hybrid MBR to GPT", DOS, GPT),
    ],
};

static MENU_CREATELABEL: Menu = Menu {
    label: 0,
    exclude: BSD,
    nonested: true,
    callback: Callback::CreateLabel,
    entries: &[
        sep("Create a new label"),
        ent(b'g', "create a new empty GPT partition table"),
        ent(b'G', "create a new empty SGI (IRIX) partition table"),
        ent(b'o', "create a new empty MBR (DOS) partition table"),
        ent(b's', "create a new empty Sun partition table"),
        // Backward compatibility: 'g' is taken, but not listed, in the
        // expert menu.
        xent_h(b'g', "create an IRIX (SGI) partition table"),
    ],
};

static MENU_GEO: Menu = Menu {
    label: 0,
    exclude: GPT | BSD,
    nonested: false,
    callback: Callback::Geo,
    entries: &[
        xsep("Geometry (for the current label)"),
        xent(b'c', "change number of cylinders"),
        xent(b'h', "change number of heads"),
        xent(b's', "change number of sectors/track"),
    ],
};

static MENU_GPT: Menu = Menu {
    label: GPT,
    exclude: 0,
    nonested: false,
    callback: Callback::Gpt,
    entries: &[
        bsep("GPT"),
        xent(b'i', "change disk GUID"),
        xent(b'n', "change partition name"),
        xent(b'u', "change partition UUID"),
        xent(b'l', "change table length"),
        bent(b'M', "enter protective/hybrid MBR"),
        xsep(""),
        xent(b'A', "toggle the legacy BIOS bootable flag"),
        xent(b'B', "toggle the no block IO protocol flag"),
        xent(b'R', "toggle the required partition flag"),
        xent(b'S', "toggle the GUID specific bits"),
    ],
};

static MENU_SUN: Menu = Menu {
    label: SUN,
    exclude: 0,
    nonested: false,
    callback: Callback::Sun,
    entries: &[
        bsep("Sun"),
        ent(b'a', "toggle the read-only flag"),
        ent(b'c', "toggle the mountable flag"),
        xent(b'a', "change number of alternate cylinders"),
        xent(b'e', "change number of extra sectors per cylinder"),
        xent(b'i', "change interleave factor"),
        xent(b'o', "change rotation speed (rpm)"),
        xent(b'y', "change number of physical cylinders"),
    ],
};

static MENU_SGI: Menu = Menu {
    label: SGI,
    exclude: 0,
    nonested: false,
    callback: Callback::Sgi,
    entries: &[
        sep("SGI"),
        ent(b'a', "select bootable partition"),
        ent(b'b', "edit bootfile entry"),
        ent(b'c', "select sgi swap partition"),
        ent(b'i', "create SGI info"),
    ],
};

static MENU_DOS: Menu = Menu {
    label: DOS,
    exclude: 0,
    nonested: false,
    callback: Callback::Dos,
    entries: &[
        bsep("DOS (MBR)"),
        ent(b'a', "toggle a bootable flag"),
        ent(b'b', "edit nested BSD disklabel"),
        ent(b'c', "toggle the dos compatibility flag"),
        xent(b'b', "move beginning of data in a partition"),
        xent(b'F', "fix partitions C/H/S values"),
        xent(b'i', "change the disk identifier"),
        bent_nest_h(b'M', "return from protective/hybrid MBR to GPT", DOS, GPT),
    ],
};

static MENU_BSD: Menu = Menu {
    label: BSD,
    exclude: 0,
    nonested: false,
    callback: Callback::Bsd,
    entries: &[
        sep("BSD"),
        ent(b'e', "edit drive data"),
        ent(b'i', "install bootstrap"),
        ent(b's', "show complete disklabel"),
        ent(b'x', "link BSD partition to non-BSD partition"),
    ],
};

/// `menus[]`, in upstream's order.
static MENUS: [&Menu; 8] = [
    &MENU_GPT,
    &MENU_SUN,
    &MENU_SGI,
    &MENU_DOS,
    &MENU_BSD,
    &MENU_GEO,
    &MENU_GENERIC,
    &MENU_CREATELABEL,
];

/// `struct menu_context`: where a walk of the menus is.
#[derive(Default)]
struct MenuContext {
    menu_idx: usize,
    entry_idx: usize,
}

/// `next_menu_entry (cxt, mc)`: the next entry the context offers, and its
/// menu.
fn next_menu_entry(
    cxt: &Context,
    mc: &mut MenuContext,
) -> Option<(&'static Menu, &'static MenuEntry)> {
    let lb = cxt.label_ref();
    let parent = cxt.get_parent();
    let ty = lb.map_or(0, |l| bits(l.id));
    let pr_type = parent
        .and_then(Context::label_ref)
        .map_or(0, |l| bits(l.id));
    let details = cxt.is_details();

    while let Some(&m) = MENUS.get(mc.menu_idx) {
        let e = m.entries.get(mc.entry_idx);
        // The whole menu: done with, for other labels, not in a nested
        // table, or excluded for this label.
        let Some(e) = e.filter(|_| {
            !((m.label != 0 && (lb.is_none() || m.label & ty == 0))
                || (m.nonested && parent.is_some())
                || (m.exclude != 0 && lb.is_some() && m.exclude & ty != 0))
        }) else {
            mc.menu_idx = mc.menu_idx.wrapping_add(1);
            mc.entry_idx = 0;
            continue;
        };
        mc.entry_idx = mc.entry_idx.wrapping_add(1);
        // The entry: excluded for this label, for other labels, for the
        // other mode, or for a table nested in another.
        if (e.exclude != 0 && lb.is_some() && e.exclude & ty != 0)
            || (e.label != 0 && (lb.is_none() || e.label & ty == 0))
            || (!e.expert && details)
            || (e.parent != 0 && (parent.is_none() || pr_type != e.parent))
            || (!e.normal && !details)
        {
            continue;
        }
        return Some((m, e));
    }
    None
}

/// `get_fdisk_menu_entry (cxt, key, &menu)`: the command `key` names here.
fn get_fdisk_menu_entry(cxt: &Context, key: u8) -> Option<(&'static Menu, &'static MenuEntry)> {
    let mut mc = MenuContext::default();
    while let Some((m, e)) = next_menu_entry(cxt, &mut mc) {
        if e.key != b'-' && e.key == key {
            return Some((m, e));
        }
    }
    None
}

impl Fdisk {
    /// `print_fdisk_menu (cxt)`: the help, the commands the context offers.
    fn print_fdisk_menu(&mut self) -> i32 {
        let term = &self.sh.term;
        term.write(if self.cxt.is_details() {
            b"\nHelp (expert commands):\n".as_slice()
        } else {
            b"\nHelp:\n"
        });
        let mut mc = MenuContext::default();
        while let Some((_, e)) = next_menu_entry(&self.cxt, &mut mc) {
            if e.hidden {
                continue;
            }
            if e.key == b'-' && e.title.is_empty() {
                term.write(b"\n");
            } else if e.key == b'-' {
                term.color_scheme_enable(b"help-title", ulcolors::BOLD);
                term.write(format!("\n  {}\n", e.title).as_bytes());
                term.color_disable();
            } else {
                term.write(format!("   {}   {}\n", char::from(e.key), e.title).as_bytes());
            }
        }
        term.write(b"\n");
        if let Some(parent) = self.cxt.get_parent() {
            let l = self.cxt.label_ref().map_or("", |l| l.get_name()).as_bytes();
            let p = parent.label_ref().map_or("", |l| l.get_name()).as_bytes();
            let m = format!(
                "You're editing nested '{}' partition table, primary partition table is '{}'.",
                escape_unprintable(l),
                escape_unprintable(p)
            );
            self.cxt.info(m.as_bytes());
        }
        0
    }

    /// `process_fdisk_menu (&cxt)`: a command read and done. 0, or <0 on an
    /// error, or what the command returned.
    pub fn process_fdisk_menu(&mut self) -> i32 {
        let prompt: &[u8] = if self.cxt.is_details() {
            b"Expert command (m for help): "
        } else {
            b"Command (m for help): "
        };
        self.sh.term.write(b"\n");
        let key = match get_user_reply(&self.sh, prompt, BUFSIZ) {
            Err(rc) if rc == -ECANCELED => {
                // ^C and ^D at the main menu are `q`.
                if self.sh.is_interactive.get()
                    && self
                        .cxt
                        .label_ref()
                        .is_some_and(libfdisk::Label::is_changed)
                {
                    match get_user_reply(
                        &self.sh,
                        b"\nAll unwritten changes will be lost, do you really want to quit? (y/n)",
                        BUFSIZ,
                    ) {
                        Ok(buf) if rpmatch(&buf) != 0 => {}
                        _ => return 0,
                    }
                }
                b'q'
            }
            Err(rc) => return rc,
            Ok(buf) => buf.first().copied().unwrap_or(0),
        };
        let Some((menu, ent)) = get_fdisk_menu_entry(&self.cxt, key) else {
            // "%c: unknown command": a NUL key ends the C string at once.
            let m = if key == 0 {
                Vec::new()
            } else {
                format!("{}: unknown command", char::from(key)).into_bytes()
            };
            self.cxt.warnx(&m);
            return -EINVAL;
        };
        match menu.callback {
            Callback::Generic => self.generic_menu_cb(ent),
            Callback::CreateLabel => self.createlabel_menu_cb(ent),
            Callback::Geo => self.geo_menu_cb(ent),
            Callback::Gpt => self.gpt_menu_cb(ent),
            Callback::Sun => self.sun_menu_cb(ent),
            Callback::Sgi => self.sgi_menu_cb(ent),
            Callback::Dos => self.dos_menu_cb(ent),
            Callback::Bsd => self.bsd_menu_cb(ent),
        }
    }

    /// `script_read (cxt)`: `I`, a layout read from an sfdisk script.
    fn script_read(&mut self) -> i32 {
        let filename = match self.cxt.ask_string(b"Enter script file name") {
            Ok(f) => f.unwrap_or_default(),
            Err(rc) => return rc,
        };
        let shown = |what: &str| {
            let mut m = what.as_bytes().to_vec();
            m.extend_from_slice(&filename);
            m
        };
        match new_script_from_file(&mut self.cxt, &filename) {
            Err(0) => {
                self.cxt.warnx(&shown("Failed to parse script file "));
                0
            }
            Err(errno) => {
                self.cxt.warn(errno, &shown("Cannot open "));
                0
            }
            Ok(sc) => {
                let sc = std::rc::Rc::new(std::cell::RefCell::new(sc));
                if self.cxt.apply_script(&sc) == 0 {
                    self.cxt.info(b"Script successfully applied.");
                    return 0;
                }
                self.cxt.warnx(&shown("Failed to apply script "));
                self.cxt.warnx(b"Resetting fdisk!");
                let mut rc = self.cxt.reassign_device();
                if rc == 0 && !self.cxt.has_label() {
                    self.cxt
                        .info(b"Device does not contain a recognized partition table.");
                    rc = self.cxt.create_disklabel(None);
                }
                rc
            }
        }
    }

    /// `script_write (cxt)`: `O`, the layout dumped as an sfdisk script.
    fn script_write(&mut self) -> i32 {
        let filename = match self.cxt.ask_string(b"Enter script file name") {
            Ok(f) => f.unwrap_or_default(),
            Err(rc) => return rc,
        };
        let mut sc = Script::new();
        let rc = sc.read_context(&mut self.cxt);
        if rc != 0 {
            self.cxt
                .warnx(b"Failed to transform disk layout into script");
            return rc;
        }
        let name_msg = |what: &str| {
            let mut m = what.as_bytes().to_vec();
            m.extend_from_slice(&filename);
            m
        };
        let mut f = match std::fs::File::create(ulsysfs::path_of(&filename)) {
            Ok(f) => f,
            Err(e) => {
                let m = name_msg("Cannot open ");
                self.cxt.warn(e.raw_os_error().unwrap_or(0), &m);
                return 0;
            }
        };
        let mut text = Vec::new();
        let rc = sc.write_file(&self.cxt, &mut text);
        // `fclose`'s status is not looked at, nor is the write's: the
        // writes go through stdio's buffer.
        let _ = std::io::Write::write_all(&mut f, &text);
        if rc != 0 {
            let m = name_msg("Failed to write script ");
            self.cxt.warn(rc.wrapping_neg(), &m);
        } else {
            self.cxt.info(b"Script successfully saved.");
        }
        rc
    }

    /// `ask_for_wipe (cxt, partno)`: a new partition's old signature,
    /// wiped by the write if the user (or `--wipe-partitions`) says so.
    fn ask_for_wipe(&mut self, partno: usize) -> i32 {
        let mut tmp = match self.cxt.get_partition(partno) {
            Ok(pa) => pa,
            Err(rc) => return rc,
        };
        let fstype = match self.cxt.partition_to_string(&mut tmp, fieldtype::FSTYPE) {
            Ok(Some(t)) => t,
            Ok(None) => return 0,
            Err(rc) => return rc,
        };
        let mut m = format!("Partition #{} contains a ", partno.wrapping_add(1)).into_bytes();
        m.extend_from_slice(&fstype);
        m.extend_from_slice(b" signature.");
        self.cxt.warnx(&m);

        let yes = if self.pwipemode == WIPEMODE_AUTO
            && std::io::IsTerminal::is_terminal(&std::io::stdin())
        {
            self.cxt
                .ask_yesno(b"Do you want to remove the signature?")
                .unwrap_or(false)
        } else {
            self.pwipemode == WIPEMODE_ALWAYS
        };
        if yes {
            self.cxt
                .info(b"The signature will be removed by a write command.");
            return self.cxt.wipe_partition(partno, true);
        }
        0
    }

    /// Leave the nested table for its parent, `Leaving nested disklabel.`
    /// said by the nested one.
    fn leave_nested(&mut self) {
        if self.cxt.get_parent().is_none() {
            return;
        }
        self.cxt.info(b"Leaving nested disklabel.");
        let child = std::mem::replace(&mut self.cxt, Context::empty());
        self.cxt = child.into_parent().unwrap_or_else(Context::empty);
    }

    /// `generic_menu_cb`: the commands every label has.
    fn generic_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        let mut rc = 0;
        // Both modes.
        match ent.key {
            b'p' => {
                list::list_disk_geometry(&mut self.cxt, &self.sh.term);
                list::list_disklabel(&mut self.cxt, &self.sh.term, &self.fields);
            }
            b'w' | b'q' => {
                if ent.key == b'w' {
                    if self.cxt.is_readonly() {
                        self.cxt.warnx(b"Device is open in read-only mode.");
                        return self.expert_or_normal(ent, rc);
                    }
                    rc = self.cxt.write_disklabel();
                    if rc != 0 {
                        ulclosestream::warn(
                            &self.sh.term.short,
                            "failed to write disklabel",
                            &std::io::Error::from_raw_os_error(rc.wrapping_neg()),
                        );
                        self.sh.term.exit(1);
                    }
                    self.cxt.info(b"The partition table has been altered.");
                    if self.cxt.get_parent().is_some() {
                        // A nested table: not left.
                        return self.expert_or_normal(ent, rc);
                    }
                    rc = if self.device_is_used {
                        self.cxt.reread_changes(&self.original_layout)
                    } else {
                        self.cxt.reread_partition_table()
                    };
                    if rc == 0 {
                        rc = self.cxt.deassign_device(false);
                    }
                }
                self.sh.term.write(b"\n");
                self.sh.term.exit(if rc == 0 { 0 } else { 1 });
            }
            b'm' => rc = self.print_fdisk_menu(),
            b'v' => rc = self.cxt.verify_disklabel(),
            b'i' => rc = self.print_partition_info(),
            b'F' => list::list_freespace(&mut self.cxt, &self.sh.term),
            _ => {}
        }
        self.expert_or_normal(ent, rc)
    }

    /// The rest of `generic_menu_cb`: the commands of one mode.
    fn expert_or_normal(&mut self, ent: &MenuEntry, mut rc: i32) -> i32 {
        if ent.expert {
            match ent.key {
                b'd' => self.dump_firstsector(),
                b'D' => self.dump_disklabel(),
                b'f' => rc = self.cxt.reorder_partitions(),
                b'r' => {
                    self.cxt.enable_details(false);
                    rc = 0;
                }
                _ => {}
            }
            return rc;
        }
        match ent.key {
            b'd' => match self.cxt.ask_partnum(false) {
                // No partitions yet, or worse: nothing to delete.
                Err(e) => rc = e,
                Ok(n) => {
                    rc = self.cxt.delete_partition(n);
                    let m = if rc == 0 {
                        format!("Partition {} has been deleted.", n.wrapping_add(1))
                    } else {
                        format!("Could not delete partition {}", n.wrapping_add(1))
                    };
                    if rc == 0 {
                        self.cxt.info(m.as_bytes());
                    } else {
                        self.cxt.warnx(m.as_bytes());
                    }
                }
            },
            b'I' => {
                self.script_read();
            }
            b'O' => {
                self.script_write();
            }
            b'l' => self.list_partition_types(),
            b'n' => {
                let mut partno = 0usize;
                rc = self.cxt.add_partition(None, Some(&mut partno));
                if rc == 0 {
                    rc = self.ask_for_wipe(partno);
                }
            }
            b't' => self.change_partition_type(),
            b'u' => {
                let to: &[u8] = if self.cxt.use_cylinders() {
                    b"sectors"
                } else {
                    b"cylinders"
                };
                self.cxt.set_unit(Some(to));
                self.cxt.info(if self.cxt.use_cylinders() {
                    b"Changing display/entry units to cylinders (DEPRECATED!).".as_slice()
                } else {
                    b"Changing display/entry units to sectors."
                });
            }
            b'x' => self.cxt.enable_details(true),
            // From a nested BSD table to the MBR, or from the MBR to the GPT.
            b'r' => self.leave_nested(),
            _ => {}
        }
        rc
    }

    /// `gpt_menu_cb`: GPT's own commands.
    fn gpt_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        if !ent.expert {
            return 0;
        }
        match ent.key {
            b'i' => return self.cxt.set_disklabel_id(),
            b'l' => {
                let n = u64::try_from(self.cxt.get_npartitions()).unwrap_or(u64::MAX);
                return match self
                    .cxt
                    .ask_number(1, n, u64::from(u32::MAX), b"New maximum entries")
                {
                    Ok(length) => libfdisk::gpt::set_npartitions(
                        &mut self.cxt,
                        libfdisk::utils::u64_as_u32(length),
                    ),
                    Err(rc) => rc,
                };
            }
            b'M' => {
                let details = self.cxt.is_details();
                let parent = std::mem::replace(&mut self.cxt, Context::empty());
                self.cxt = Context::new_nested(parent, Some(b"dos"));
                if details {
                    // Still in expert mode.
                    self.cxt.enable_details(true);
                }
                self.cxt.info(b"Entering protective/hybrid MBR disklabel.");
                return 0;
            }
            _ => {}
        }
        // The rest take a partition.
        let n = match self.cxt.ask_partnum(false) {
            Ok(n) => n,
            Err(rc) => return rc,
        };
        match ent.key {
            b'u' | b'n' => {
                let q: &[u8] = if ent.key == b'u' {
                    b"New UUID (in 8-4-4-4-12 format)"
                } else {
                    b"New name"
                };
                let s = match self.cxt.ask_string(q) {
                    Ok(s) => s,
                    Err(rc) => return rc,
                };
                // A new template.
                let mut pa = Partition::new();
                if ent.key == b'u' {
                    pa.set_uuid(s.as_deref());
                } else {
                    pa.set_name(s.as_deref());
                }
                self.cxt.set_partition(n, &mut pa)
            }
            b'A' => self
                .cxt
                .toggle_partition_flag(n, libfdisk::GPT_FLAG_LEGACYBOOT),
            b'B' => self
                .cxt
                .toggle_partition_flag(n, libfdisk::GPT_FLAG_NOBLOCK),
            b'R' => self
                .cxt
                .toggle_partition_flag(n, libfdisk::GPT_FLAG_REQUIRED),
            b'S' => self
                .cxt
                .toggle_partition_flag(n, libfdisk::GPT_FLAG_GUIDSPECIFIC),
            _ => 0,
        }
    }

    /// `dos_menu_cb`: the MBR's own commands.
    fn dos_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        if !ent.expert {
            return match ent.key {
                b'a' => match self.cxt.ask_partnum(false) {
                    Ok(n) => self.cxt.toggle_partition_flag(n, libfdisk::DOS_FLAG_ACTIVE),
                    Err(rc) => rc,
                },
                b'b' => {
                    let parent = std::mem::replace(&mut self.cxt, Context::empty());
                    let mut bsd = Context::new_nested(parent, Some(b"bsd"));
                    let rc = if bsd.has_label() {
                        0
                    } else {
                        bsd.create_disklabel(Some(b"bsd"))
                    };
                    if rc == 0 {
                        self.cxt = bsd;
                        self.cxt.info(b"Entering nested BSD disklabel.");
                    } else {
                        self.cxt = bsd.into_parent().unwrap_or_else(Context::empty);
                    }
                    rc
                }
                b'c' => {
                    self.toggle_dos_compatibility_flag();
                    0
                }
                _ => 0,
            };
        }
        match ent.key {
            b'b' => match self.cxt.ask_partnum(false) {
                Ok(n) => libfdisk::dos::move_begin(&mut self.cxt, n),
                Err(rc) => rc,
            },
            b'i' => self.cxt.set_disklabel_id(),
            // From the nested MBR to the GPT (backward compatibility only).
            b'M' => {
                self.leave_nested();
                0
            }
            b'F' => {
                let rc = libfdisk::dos::fix_chs(&mut self.cxt);
                self.cxt.info(if rc != 0 {
                    b"C/H/S values fixed.".as_slice()
                } else {
                    b"Nothing to do. C/H/S values are correct already."
                });
                rc
            }
            _ => 0,
        }
    }

    /// `sun_menu_cb`: Sun's own commands.
    fn sun_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        if !ent.expert {
            let n = match self.cxt.ask_partnum(false) {
                Ok(n) => n,
                Err(rc) => return rc,
            };
            return match ent.key {
                b'a' => self.cxt.toggle_partition_flag(n, libfdisk::sun::FLAG_RONLY),
                b'c' => self.cxt.toggle_partition_flag(n, libfdisk::sun::FLAG_UNMNT),
                _ => 0,
            };
        }
        match ent.key {
            b'a' => libfdisk::sun::set_alt_cyl(&mut self.cxt),
            b'e' => libfdisk::sun::set_xcyl(&mut self.cxt),
            b'i' => libfdisk::sun::set_ilfact(&mut self.cxt),
            b'o' => libfdisk::sun::set_rspeed(&mut self.cxt),
            b'y' => libfdisk::sun::set_pcylcount(&mut self.cxt),
            _ => 0,
        }
    }

    /// `sgi_menu_cb`: SGI's own commands -- `EINVAL` unless one changes
    /// something, as upstream returns.
    fn sgi_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        if ent.expert {
            return -EINVAL;
        }
        match ent.key {
            b'a' | b'c' => match self.cxt.ask_partnum(false) {
                Ok(n) => {
                    let flag = if ent.key == b'a' {
                        libfdisk::SGI_FLAG_BOOT
                    } else {
                        libfdisk::SGI_FLAG_SWAP
                    };
                    self.cxt.toggle_partition_flag(n, flag)
                }
                Err(rc) => rc,
            },
            b'b' => {
                // Its result is not looked at.
                let _ = libfdisk::sgi::set_bootfile(&mut self.cxt);
                -EINVAL
            }
            b'i' => libfdisk::sgi::create_info(&mut self.cxt),
            _ => -EINVAL,
        }
    }

    /// `bsd_menu_cb`: BSD's own commands.
    fn bsd_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        match ent.key {
            b'e' => libfdisk::bsd::edit_disklabel(&mut self.cxt),
            b'i' => libfdisk::bsd::write_bootstrap(&mut self.cxt),
            b's' => {
                let org = self.cxt.is_details();
                self.cxt.enable_details(true);
                list::list_disklabel(&mut self.cxt, &self.sh.term, &self.fields);
                self.cxt.enable_details(org);
                0
            }
            b'x' => libfdisk::bsd::link_partition(&mut self.cxt),
            _ => 0,
        }
    }

    /// `geo_menu_cb`: the C/H/S the label works with, changed until the
    /// device's properties are read again (a new label, say).
    fn geo_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        let li = self
            .cxt
            .get_label(None)
            .or_else(|| self.cxt.get_label(Some(b"dos")));
        let Some(lb) = li.and_then(|i| self.cxt.label_at(i)) else {
            return -EINVAL;
        };
        let (mut c, mut h, mut s) = (0u64, 0u64, 0u64);
        let rc = match ent.key {
            b'c' => {
                let (mi, ma) = lb.get_geomrange_cylinders().unwrap_or((0, 0));
                let dflt = self.cxt.get_geom_cylinders();
                self.cxt
                    .ask_number(mi, dflt, ma, b"Number of cylinders")
                    .map(|v| c = v)
            }
            b'h' => {
                let (mi, ma) = lb.get_geomrange_heads().unwrap_or((0, 0));
                let dflt = u64::from(self.cxt.get_geom_heads());
                self.cxt
                    .ask_number(u64::from(mi), dflt, u64::from(ma), b"Number of heads")
                    .map(|v| h = v)
            }
            b's' => {
                let (mi, ma) = lb.get_geomrange_sectors().unwrap_or((0, 0));
                let dflt = self.cxt.get_geom_sectors();
                self.cxt
                    .ask_number(mi, dflt, ma, b"Number of sectors")
                    .map(|v| s = v)
            }
            _ => Err(-EINVAL),
        };
        match rc {
            Ok(()) => {
                use libfdisk::utils::u64_as_u32;
                self.cxt
                    .override_geometry(u64_as_u32(c), u64_as_u32(h), u64_as_u32(s));
                0
            }
            Err(rc) => rc,
        }
    }

    /// `createlabel_menu_cb`: a new, empty table of a kind.
    fn createlabel_menu_cb(&mut self, ent: &MenuEntry) -> i32 {
        let wanted: Option<&str> = if ent.expert {
            // Deprecated: 'G' in the main menu.
            (ent.key == b'g').then_some("sgi")
        } else {
            match ent.key {
                b'g' => Some("gpt"),
                b'G' => Some("sgi"),
                b'o' => Some("dos"),
                b's' => Some("sun"),
                _ => None,
            }
        };
        let mut rc = -EINVAL;
        if let Some(wanted) = wanted {
            rc = self.cxt.create_disklabel(Some(wanted.as_bytes()));
            if rc != 0 {
                let shown = wanted.as_bytes();
                let m = format!(
                    "Failed to create '{}' disk label",
                    escape_unprintable(shown)
                );
                self.cxt.warn(rc.wrapping_neg(), m.as_bytes());
            }
        }
        if rc == 0 && self.cxt.get_collision().is_some() {
            self.follow_wipe_mode();
        }
        rc
    }
}

#[cfg(test)]
pub mod tests_support {
    use super::{Context, MenuContext, next_menu_entry};

    /// The keys of the commands `cxt` offers, separators aside, in the
    /// order the help lists them.
    pub fn walk(cxt: &Context) -> Vec<u8> {
        let mut mc = MenuContext::default();
        let mut keys = Vec::new();
        while let Some((_, e)) = next_menu_entry(cxt, &mut mc) {
            if e.key != b'-' {
                keys.push(e.key);
            }
        }
        keys
    }
}
