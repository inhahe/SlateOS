"""One-shot: reminders production lint findings."""

import pathlib

p = pathlib.Path("apps/reminders/src/main.rs")
s = p.read_text(encoding="utf-8")


def sub(old, new, what, n=1):
    global s
    assert s.count(old) >= n, f"anchor: {what}"
    s = s.replace(old, new, n)
    print("ok:", what)


sub(
    "        self.hour * 60 + self.minute",
    """        // Saturating rather than wrapping: an hour past 23 is bad data, and
        // a wrapped minute-of-day sorts a task to the wrong end of the list.
        self.hour.saturating_mul(60).saturating_add(self.minute)""",
    "to_minutes",
)
sub('            (self.hour - 12, "PM")', '            (self.hour.saturating_sub(12), "PM")', "12h format")
sub(
    "        let minute_diff = i64::from(self.time.to_minutes()) - i64::from(other.time.to_minutes());",
    """        let minute_diff =
            i64::from(self.time.to_minutes()).saturating_sub(i64::from(other.time.to_minutes()));""",
    "minute_diff",
)
sub(
    "                diff >= 0 && diff % i64::from(*interval_days) == 0",
    """                // `checked_rem` although the zero case is handled above:
                // the guard and the division are three lines apart, and this
                // is what keeps them from drifting.
                diff >= 0 && diff.checked_rem(i64::from(*interval_days)) == Some(0)""",
    "recurrence",
)
sub(
    "        (done * 100).checked_div(total).unwrap_or(0)",
    "        done.saturating_mul(100).checked_div(total).unwrap_or(0)",
    "subtask percent",
)
sub(
    """        let total_minutes = now.time.to_minutes() + duration.as_minutes();
        let extra_days = total_minutes / 1440;
        let remaining = total_minutes % 1440;
        let new_date = now.date.add_days(extra_days as i32);""",
    """        let total_minutes = now.time.to_minutes().saturating_add(duration.as_minutes());
        let extra_days = total_minutes / 1440;
        let remaining = total_minutes % 1440;
        // A snooze that would push the date past `i32` days is not a snooze;
        // clamping keeps it at the far future rather than wrapping to the past.
        let new_date = now.date.add_days(i32::try_from(extra_days).unwrap_or(i32::MAX));""",
    "snooze",
)
sub(
    '                format!("{} days ago", -diff)',
    '                format!("{} days ago", diff.saturating_neg())',
    "days ago",
)
sub(
    "        self.next_id += 1;",
    """        // Saturating: a wrapped counter hands out an id that already exists,
        // and both selection and completion are by id.
        self.next_id = self.next_id.saturating_add(1);""",
    "next_id",
)
sub("            count += 1;", "            count = count.saturating_add(1);", "import count")
sub("        depth += 1;", "        depth = depth.saturating_add(1);", "json depth", n=99)

p.write_text(s, encoding="utf-8", newline="\n")
print("done")
