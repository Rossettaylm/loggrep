//! Session bookmark compare tray: owned log snapshots + jump `row_id`.

use crate::model::EntryRow;

/// Soft cap on pins in the session tray.
pub const BOOKMARK_SOFT_CAP: usize = 16;

#[derive(Debug, Clone)]
pub struct Bookmark {
    /// Owned snapshot at pin time (display source). `row.row_id` is the jump id.
    pub row: EntryRow,
}

impl Bookmark {
    pub fn from_row(row: EntryRow) -> Self {
        Self { row }
    }

    pub fn row_id(&self) -> u64 {
        self.row.row_id
    }
}

#[derive(Debug, Default)]
pub struct BookmarkList {
    /// Insert order (oldest → newest). Display order is [`Self::sorted_indices`].
    pub items: Vec<Bookmark>,
}

impl BookmarkList {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains_id(&self, row_id: u64) -> bool {
        self.items.iter().any(|b| b.row_id() == row_id)
    }

    /// Push if under cap and not duplicate.
    pub fn try_add(&mut self, bm: Bookmark) -> Result<(), AddError> {
        if self.contains_id(bm.row_id()) {
            return Err(AddError::Duplicate);
        }
        if self.items.len() >= BOOKMARK_SOFT_CAP {
            return Err(AddError::Full);
        }
        self.items.push(bm);
        Ok(())
    }

    pub fn remove_id(&mut self, row_id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|b| b.row_id() != row_id);
        self.items.len() < before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn delete_at(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.items.remove(index);
        true
    }

    /// Storage indices in compare order: `time_full` ascending, then `row_id`.
    /// Pins with no parseable time sit last (stable by `row_id`).
    pub fn sorted_indices(&self) -> Vec<usize> {
        let mut idxs: Vec<usize> = (0..self.items.len()).collect();
        idxs.sort_by(|&a, &b| {
            let ea = self.items[a].row.as_log_entry();
            let eb = self.items[b].row.as_log_entry();
            match (ea.time_full(), eb.time_full()) {
                (Some(ta), Some(tb)) => ta
                    .cmp(tb)
                    .then_with(|| self.items[a].row_id().cmp(&self.items[b].row_id())),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => self.items[a].row_id().cmp(&self.items[b].row_id()),
            }
        });
        idxs
    }

    /// One-line Log-top summary: `★ N` plus optional timed span. Empty list → empty.
    pub fn summary_line(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let n = self.len();
        let timed: Vec<(String, String)> = self
            .sorted_indices()
            .into_iter()
            .filter_map(|i| {
                let e = self.items[i].row.as_log_entry();
                Some((e.time_full()?.to_string(), e.time_hms()?.to_string()))
            })
            .collect();
        if timed.is_empty() {
            return format!("★ {n}");
        }
        let (min_full, min_hms) = &timed[0];
        let (max_full, max_hms) = &timed[timed.len() - 1];
        let span = summary_span(min_full, min_hms, max_full, max_hms);
        format!("★ {n}  {span}")
    }

    /// Δt labels in [`Self::sorted_indices`] order.
    /// `None` = first timed pin (hide Δt); `Some("—")` untimed; `Some("+1.2s")` timed.
    pub fn delta_labels(&self) -> Vec<Option<String>> {
        let mut out = Vec::with_capacity(self.items.len());
        let mut prev_ms: Option<i64> = None;
        for &i in &self.sorted_indices() {
            let Some(ms) = stamp_millis(&self.items[i].row) else {
                out.push(Some("—".to_string()));
                continue;
            };
            match prev_ms {
                None => out.push(None),
                Some(prev) => {
                    let delta = ms.saturating_sub(prev).max(0) as u64;
                    out.push(Some(format_delta(delta)));
                }
            }
            prev_ms = Some(ms);
        }
        out
    }
}

/// Compare-tray modal (not a `PickerSession`). Cursor indexes [`BookmarkList::sorted_indices`].
#[derive(Debug, Clone)]
pub struct ComparePanel {
    pub cursor: usize,
    pub list_offset: usize,
    pub pending_d: bool,
    pub pending_y: bool,
}

impl ComparePanel {
    pub fn new() -> Self {
        Self {
            cursor: 0,
            list_offset: 0,
            pending_d: false,
            pending_y: false,
        }
    }

    pub fn clear_pending(&mut self) {
        self.pending_d = false;
        self.pending_y = false;
    }

    pub fn clamp_cursor(&mut self, len: usize) {
        if len == 0 {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(len - 1);
        }
    }

    pub fn move_by(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let next = self.cursor as isize + delta;
        self.cursor = next.clamp(0, (len as isize) - 1) as usize;
        self.clear_pending();
    }
}

impl Default for ComparePanel {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddError {
    Duplicate,
    Full,
}

/// Result of jumping to a bookmarked `row_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpResult {
    Ok,
    /// Row left the ring buffer.
    Evicted,
    /// Row still buffered but not in current `visible`.
    Filtered,
}

/// Build a compact label from log fields (tests / fallback). Msg kept in full.
pub fn bookmark_label(timestamp: &str, level: char, tag: &str, msg: &str) -> String {
    let msg = msg.trim_end();
    let tag = if tag.is_empty() { "-" } else { tag };
    if msg.is_empty() {
        format!("{timestamp} {level} {tag}")
    } else {
        format!("{timestamp} {level} {tag} {msg}")
    }
}

/// Fit `label` into `max_cols` display cells, appending `…` when truncated.
pub fn fit_label(label: &str, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }
    let count = label.chars().count();
    if count <= max_cols {
        return label.to_string();
    }
    if max_cols == 1 {
        return "…".to_string();
    }
    let mut out: String = label.chars().take(max_cols - 1).collect();
    out.push('…');
    out
}

fn summary_span(min_full: &str, min_hms: &str, max_full: &str, max_hms: &str) -> String {
    let min_date = date_part(min_full);
    let max_date = date_part(max_full);
    match (min_date, max_date) {
        (Some(a), Some(b)) if a != b => format!("{a} {min_hms}→{b} {max_hms}"),
        _ => format!("{min_hms}→{max_hms}"),
    }
}

fn date_part(full: &str) -> Option<&str> {
    if full.len() >= 19 && full.as_bytes()[4] == b'-' {
        Some(&full[..10])
    } else if full.len() >= 5 && full.as_bytes().get(2) == Some(&b'-') {
        Some(&full[..5])
    } else {
        None
    }
}

fn stamp_millis(row: &EntryRow) -> Option<i64> {
    let entry = row.as_log_entry();
    let full = entry.time_full()?;
    let hms = entry.time_hms()?;
    if hms.len() < 8 {
        return None;
    }
    let hh: i64 = hms[0..2].parse().ok()?;
    let mm: i64 = hms[3..5].parse().ok()?;
    let ss: i64 = hms[6..8].parse().ok()?;
    let mut ms = (hh * 3600 + mm * 60 + ss) * 1000;
    ms += extra_millis(&row.timestamp) as i64;
    if let Some(days) = date_days(full) {
        ms += days * 86_400_000;
    }
    Some(ms)
}

fn extra_millis(ts: &str) -> u64 {
    let rest = if ts.len() >= 19 && ts.as_bytes().get(4) == Some(&b'-') {
        ts.get(19..)
    } else if ts.len() >= 14 {
        ts.get(14..)
    } else {
        None
    };
    let Some(rest) = rest else {
        return 0;
    };
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    let digits: String = rest
        .chars()
        .take(3)
        .filter(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return 0;
    }
    let n: u64 = digits.parse().unwrap_or(0);
    match digits.len() {
        1 => n * 100,
        2 => n * 10,
        _ => n,
    }
}

fn date_days(full: &str) -> Option<i64> {
    if full.len() >= 19 && full.as_bytes()[4] == b'-' {
        let y: i64 = full[0..4].parse().ok()?;
        let m: i64 = full[5..7].parse().ok()?;
        let d: i64 = full[8..10].parse().ok()?;
        Some(m * 31 + d + y * 366)
    } else if full.len() >= 5 && full.as_bytes().get(2) == Some(&b'-') {
        let m: i64 = full[0..2].parse().ok()?;
        let d: i64 = full[3..5].parse().ok()?;
        Some(m * 31 + d)
    } else {
        None
    }
}

fn format_delta(ms: u64) -> String {
    if ms < 60_000 {
        let secs = ms as f64 / 1000.0;
        if (secs - secs.round()).abs() < 0.05 {
            format!("+{:.0}s", secs.round())
        } else {
            format!("+{secs:.1}s")
        }
    } else {
        let total_secs = ms / 1000;
        let h = total_secs / 3600;
        let m = (total_secs % 3600) / 60;
        if h > 0 {
            if m > 0 {
                format!("+{h}h{m}m")
            } else {
                format!("+{h}h")
            }
        } else {
            format!("+{m}m")
        }
    }
}

/// Pairwise helper used by tests: `None` hide (first timed), `Some("—")` untimed.
pub fn delta_label(prev_ts: Option<&str>, curr_ts: &str) -> Option<String> {
    let curr = dummy_row(0, curr_ts);
    let curr_ms = stamp_millis(&curr);
    if curr_ms.is_none() {
        return Some("—".to_string());
    }
    let Some(prev) = prev_ts else {
        return None;
    };
    let prev_row = dummy_row(0, prev);
    let Some(prev_ms) = stamp_millis(&prev_row) else {
        return None;
    };
    let delta = curr_ms.unwrap().saturating_sub(prev_ms).max(0) as u64;
    Some(format_delta(delta))
}

fn dummy_row(row_id: u64, ts: &str) -> EntryRow {
    let line = if ts.is_empty() {
        "not a log line".to_string()
    } else if ts.len() >= 19 && ts.as_bytes().get(4) == Some(&b'-') {
        format!("{ts} 1234 5678 I Tag     : msg")
    } else {
        format!("{ts}  1  1 I Tag     : msg")
    };
    let mut row = EntryRow::from_line_or_raw(&line);
    if ts.is_empty() {
        row.timestamp.clear();
    } else {
        row.timestamp = ts.to_string();
    }
    row.row_id = row_id;
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm_ts(row_id: u64, ts: &str) -> Bookmark {
        Bookmark::from_row(dummy_row(row_id, ts))
    }

    #[test]
    fn try_add_dedup_and_cap() {
        let mut list = BookmarkList::default();
        assert!(list.try_add(bm_ts(1, "04-02 10:00:00.000")).is_ok());
        assert_eq!(
            list.try_add(bm_ts(1, "04-02 10:00:00.000")).err(),
            Some(AddError::Duplicate)
        );
        for i in 2..=BOOKMARK_SOFT_CAP as u64 {
            assert!(list.try_add(bm_ts(i, "04-02 10:00:00.000")).is_ok());
        }
        assert_eq!(
            list.try_add(bm_ts(9999, "04-02 10:00:00.000")).err(),
            Some(AddError::Full)
        );
        assert_eq!(list.len(), BOOKMARK_SOFT_CAP);
    }

    #[test]
    fn sorted_indices_time_then_row_id_untimed_last() {
        let mut list = BookmarkList::default();
        list.try_add(bm_ts(3, "04-02 10:00:08.000")).unwrap();
        list.try_add(bm_ts(1, "04-02 10:00:01.000")).unwrap();
        list.try_add(bm_ts(9, "")).unwrap();
        list.try_add(bm_ts(2, "04-02 10:00:01.000")).unwrap();
        let order: Vec<u64> = list
            .sorted_indices()
            .into_iter()
            .map(|i| list.items[i].row_id())
            .collect();
        assert_eq!(order, vec![1, 2, 3, 9]);
    }

    #[test]
    fn delta_vs_previous_timed_untimed_em_dash() {
        let mut list = BookmarkList::default();
        list.try_add(bm_ts(1, "04-02 10:00:00.000")).unwrap();
        list.try_add(bm_ts(2, "04-02 10:00:01.200")).unwrap();
        list.try_add(bm_ts(3, "")).unwrap();
        let labels = list.delta_labels();
        assert_eq!(labels[0], None);
        assert_eq!(labels[1].as_deref(), Some("+1.2s"));
        assert_eq!(labels[2].as_deref(), Some("—"));
    }

    #[test]
    fn delta_label_helper_first_timed_hides() {
        assert_eq!(delta_label(None, "04-02 10:00:00.000"), None);
        assert_eq!(
            delta_label(Some("04-02 10:00:00.000"), "04-02 10:03:00.000").as_deref(),
            Some("+3m")
        );
        assert_eq!(
            delta_label(Some("04-02 10:00:00.000"), "").as_deref(),
            Some("—")
        );
    }

    #[test]
    fn summary_same_day_hms_only() {
        let mut list = BookmarkList::default();
        list.try_add(bm_ts(2, "04-02 10:01:08.000")).unwrap();
        list.try_add(bm_ts(1, "04-02 10:01:02.000")).unwrap();
        assert_eq!(list.summary_line(), "★ 2  10:01:02→10:01:08");
    }

    #[test]
    fn summary_cross_day_includes_mm_dd() {
        let mut list = BookmarkList::default();
        list.try_add(bm_ts(1, "04-02 10:00:00.000")).unwrap();
        list.try_add(bm_ts(2, "04-03 11:00:00.000")).unwrap();
        assert_eq!(list.summary_line(), "★ 2  04-02 10:00:00→04-03 11:00:00");
    }

    #[test]
    fn summary_xlog_cross_day_includes_year() {
        let mut list = BookmarkList::default();
        list.try_add(bm_ts(1, "2026-04-02 10:00:00.000")).unwrap();
        list.try_add(bm_ts(2, "2026-04-03 11:00:00.000")).unwrap();
        assert_eq!(
            list.summary_line(),
            "★ 2  2026-04-02 10:00:00→2026-04-03 11:00:00"
        );
    }

    #[test]
    fn summary_zero_timed_is_count_only() {
        let mut list = BookmarkList::default();
        list.try_add(bm_ts(1, "")).unwrap();
        list.try_add(bm_ts(2, "")).unwrap();
        assert_eq!(list.summary_line(), "★ 2");
    }

    #[test]
    fn bookmark_label_keeps_long_text() {
        let msg = "x".repeat(80);
        let label = bookmark_label("04-02 10:00:00.000", 'I', "Tag", &msg);
        assert!(label.len() > 56, "must not eagerly truncate at 56");
        assert!(label.starts_with("04-02 10:00:00.000 I Tag "));
        assert_eq!(
            label.chars().count(),
            "04-02 10:00:00.000 I Tag ".chars().count() + 80
        );
    }

    #[test]
    fn fit_label_truncates_with_ellipsis() {
        assert_eq!(fit_label("abcdef", 4), "abc…");
        assert_eq!(fit_label("ab", 4), "ab");
        assert_eq!(fit_label("ab", 0), "");
    }
}
