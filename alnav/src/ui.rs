use std::sync::OnceLock;

use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
#[cfg(test)]
use ratatui::style::Color;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use alnav::crash::{CrashDetector, CrashInfo, CrashType};
use alnav::parser::LogEntry;

use crate::app::{App, Focus, Mode};
use crate::filter_model::Group;
use crate::fuzzy::PaintField;
use crate::highlight_model::{HighlightBox, HighlightGroup};
use crate::input::{ChipField, InputBox};
use crate::model::EntryRow;
use crate::theme;

/// Horizontal gap (columns) between chip groups on the same wrap row.
const CHIP_GROUP_GAP: u16 = 1;
/// Gap between the selection marker and the group's pills.
const DOT_PILL_GAP: u16 = 1;
/// Gap between adjacent pills inside a group.
const PILL_GAP: u16 = 1;
/// Shared centered-modal width: leave 2 cols margin each side, clamp to a
/// readable band so Input and Search share one visual scale.
pub const MODAL_WIDTH_MIN: u16 = 24;
pub const MODAL_WIDTH_MAX: u16 = 56;
/// fzf picker outer frame: leave 2 cols margin each side.
const PICKER_FRAME_WIDTH_MARGIN: u16 = 4;
/// fzf picker height ≈ 75% of frame, clamped to this minimum.
const PICKER_FRAME_MIN_HEIGHT: u16 = 10;
/// Compact (no-preview) picker minimum height — lower so half-height can shrink.
const PICKER_FRAME_COMPACT_MIN_HEIGHT: u16 = 6;
/// Minimum width for each left/right pane inside the picker.
const PICKER_LR_MIN_WIDTH: u16 = 10;
/// Rounded search input height at the bottom of the left pane.
const PICKER_SEARCH_HEIGHT: u16 = 3;
/// Horizontal padding between the search border and its content.
const PICKER_SEARCH_HORIZONTAL_PADDING: u16 = 1;
/// Gap between adjacent popup surfaces (Picker L/R, modal → candidates → Preview).
const POPUP_GAP: u16 = 1;
/// LogList tag column width (display columns); short tags pad, long tags truncate.
const TAG_COL_WIDTH: usize = 20;
/// Preview pane tag column cap (narrower than LogList so msg keeps budget).
const PREVIEW_TAG_COL_MAX: usize = 12;
/// Floor for the tag column when the pane is narrow (still may shrink further).
const TAG_COL_MIN: usize = 4;
/// Left/right split when the picker shows a Preview pane (right-biased).
pub const PICKER_PREVIEW_LEFT_RATIO: f32 = 0.3;
/// Gap between level badge and tag column (outside badge fill).
const LEVEL_TAG_GAP: usize = 1;
/// Gap between the fixed tag column and the message.
const TAG_MSG_GAP: usize = 1;
/// Per-row action icon for candidate lists (F3). `None` = no icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    None,
    Jump,
    Toggle { enabled: bool },
}

impl ActionKind {
    fn icon(self) -> &'static str {
        match self {
            ActionKind::None => "",
            ActionKind::Jump => theme::GLYPH_ACTION_JUMP,
            ActionKind::Toggle { enabled: true } => theme::GLYPH_ACTION_TOGGLE_ON,
            ActionKind::Toggle { enabled: false } => theme::GLYPH_ACTION_TOGGLE_OFF,
        }
    }

    fn icon_style(self) -> Style {
        match self {
            ActionKind::None => Style::default(),
            ActionKind::Jump => Style::default().fg(theme::accent()),
            ActionKind::Toggle { enabled: true } => Style::default().fg(theme::success()),
            ActionKind::Toggle { enabled: false } => theme::disabled_chip_style(),
        }
    }
}

fn rounded_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style(active))
        .style(theme::canvas_style())
        .title(title)
}

/// Top/bottom-only divider block (Q3 path B: weakened borders). Uses
/// box-drawing `─` (U+2500) for horizontal rules; no left/right borders,
/// giving the inner content 2 extra columns vs `rounded_block`.
fn divider_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_type(BorderType::Plain)
        .border_style(theme::border_style(active))
        .style(theme::canvas_style())
        .title(title)
}

/// Unified width for centered Input / Search modals.
pub fn modal_width(frame_width: u16) -> u16 {
    frame_width
        .saturating_sub(4)
        .clamp(MODAL_WIDTH_MIN, MODAL_WIDTH_MAX)
}

/// Horizontally and vertically center a `width`×`height` rect inside `frame`.
pub fn centered_modal_rect(frame: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(frame.width).max(1);
    let height = height.min(frame.height).max(1);
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame.y + (frame.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Horizontally centered, vertically near the top (H1 Input/Search stack).
pub fn top_modal_rect(frame: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(frame.width).max(1);
    let height = height.min(frame.height).max(1);
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame
        .y
        .saturating_add(1)
        .min(frame.y.saturating_add(frame.height.saturating_sub(height)));
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Place a rect of `height` directly below `anchor`, clamped to `frame`.
pub fn stack_below_rect(anchor: Rect, frame: Rect, height: u16) -> Rect {
    let y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space = frame_bottom.saturating_sub(y);
    let height = height.min(space).max(if space > 0 { 1 } else { 0 });
    Rect {
        x: anchor.x,
        y,
        width: anchor.width,
        height,
    }
}

/// Like [`stack_below_rect`], but leave [`POPUP_GAP`] rows when there is room;
/// if the remaining space is ≤ gap, pack flush (no gap) so a 1-row sliver still fits.
pub fn stack_below_rect_gapped(anchor: Rect, frame: Rect, height: u16) -> Rect {
    let flush_y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space_flush = frame_bottom.saturating_sub(flush_y);
    if space_flush > POPUP_GAP {
        let gapped_anchor = Rect {
            x: anchor.x,
            y: anchor.y,
            width: anchor.width,
            height: anchor.height.saturating_add(POPUP_GAP),
        };
        stack_below_rect(gapped_anchor, frame, height)
    } else {
        stack_below_rect(anchor, frame, height)
    }
}

/// Horizontal center, height ≈ 75% of `frame` (clamped to a readable minimum).
/// When `show_preview` is false, width is ≈ half of the full picker width and
/// height is ≈ half of the full picker height (≈ 3/8 of frame).
pub fn picker_frame_rect(frame: Rect, show_preview: bool) -> Rect {
    let full_w = frame
        .width
        .saturating_sub(PICKER_FRAME_WIDTH_MARGIN)
        .max(PICKER_LR_MIN_WIDTH.saturating_mul(2));
    let width = if show_preview {
        full_w
    } else {
        (full_w / 2).max(PICKER_LR_MIN_WIDTH)
    };
    let full_height = (frame.height * 3 / 4)
        .max(PICKER_FRAME_MIN_HEIGHT)
        .min(frame.height);
    let height = if show_preview {
        full_height
    } else {
        (full_height / 2)
            .max(PICKER_FRAME_COMPACT_MIN_HEIGHT)
            .min(frame.height)
    };
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame.y + (frame.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Split `area` into left/right panes by `left_ratio`; each pane is at least
/// [`PICKER_LR_MIN_WIDTH`] columns wide.
pub fn split_picker_lr(area: Rect, left_ratio: f32) -> (Rect, Rect) {
    let total = area.width;
    let mut left_w = ((total as f32) * left_ratio).round() as u16;
    left_w = left_w
        .max(PICKER_LR_MIN_WIDTH)
        .min(total.saturating_sub(PICKER_LR_MIN_WIDTH));
    let right_w = total.saturating_sub(left_w);
    let left = Rect {
        x: area.x,
        y: area.y,
        width: left_w,
        height: area.height,
    };
    let right = Rect {
        x: area.x + left_w,
        y: area.y,
        width: right_w,
        height: area.height,
    };
    (left, right)
}

/// Like [`split_picker_lr`], but leave [`POPUP_GAP`] columns between panes.
pub fn split_picker_lr_gapped(area: Rect, left_ratio: f32) -> (Rect, Rect) {
    let gap = POPUP_GAP.min(
        area.width
            .saturating_sub(PICKER_LR_MIN_WIDTH.saturating_mul(2)),
    );
    let usable = Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(gap),
        height: area.height,
    };
    let (left, right_inner) = split_picker_lr(usable, left_ratio);
    let right = Rect {
        x: left.x.saturating_add(left.width).saturating_add(gap),
        y: area.y,
        width: right_inner.width,
        height: area.height,
    };
    (left, right)
}

/// Left pane vertical stack: candidates fill the top, search area pinned to bottom.
/// `chip_rows` is the committed-chip band height above the rounded search input
/// (0 when empty); search total height is `PICKER_SEARCH_HEIGHT + chip_rows`.
pub fn picker_left_stack(left: Rect, chip_rows: u16) -> (Rect, Rect) {
    let search_h = PICKER_SEARCH_HEIGHT
        .saturating_add(chip_rows)
        .min(left.height);
    let cand_h = left.height.saturating_sub(search_h);
    let candidates = Rect {
        x: left.x,
        y: left.y,
        width: left.width,
        height: cand_h,
    };
    let search = Rect {
        x: left.x,
        y: left.y + cand_h,
        width: left.width,
        height: search_h,
    };
    (candidates, search)
}

/// Rounded four-sided popup shell with a glyph-prefixed plain title.
/// Returns the inner content rect.
fn popup_block(title: &str) -> Block<'static> {
    rounded_block(
        theme::plain_title(theme::GLYPH_TITLE_PICKER, title, true),
        true,
    )
}

fn clear_to_canvas(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::canvas_style()), area);
}

/// Clear + rounded full-border shell (dim accent). Returns the inner content rect.
pub fn render_modal_shell(title: &str, frame: &mut Frame, area: Rect) -> Rect {
    render_modal_shell_glyph(theme::GLYPH_TITLE_PICKER, title, frame, area)
}

/// Like [`render_modal_shell`] but with an explicit title glyph.
pub fn render_modal_shell_glyph(
    glyph: &'static str,
    title: &str,
    frame: &mut Frame,
    area: Rect,
) -> Rect {
    clear_to_canvas(frame, area);
    let block = rounded_block(theme::plain_title(glyph, title, true), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    clear_to_canvas(frame, inner);
    inner
}

/// Build label spans with optional fuzzy match coloring (Pattern atoms / multi-range).
/// `checked` changes the selection-marker (prefix) color for Tab multi-select.
fn candidate_label_spans_with_scorer(
    label: &str,
    scorer: &mut crate::fuzzy::FuzzyScorer,
    selected: bool,
    checked: bool,
    base: Style,
    action: ActionKind,
    area_width: u16,
) -> Vec<Span<'static>> {
    use crate::bookmark::fit_label;
    use crate::fuzzy;
    let match_style = theme::candidate_match_style(selected);
    let prefix = if selected || checked {
        theme::candidate_prefix()
    } else {
        " ".repeat(theme::candidate_prefix().chars().count().max(1))
    };
    let prefix_style = if checked {
        theme::candidate_checked_prefix_style().bg(base.bg.unwrap_or(theme::canvas_bg()))
    } else {
        base
    };
    // icon occupies 1 glyph + 1 trailing pad when present.
    let icon_glyph = action.icon();
    let icon_w: u16 = if icon_glyph.is_empty() { 0 } else { 2 };
    let prefix_len = prefix.chars().count() as u16;
    // label budget = area − prefix − icon+pad − 1 trailing pad
    let label_max = (area_width as usize)
        .saturating_sub(prefix_len as usize)
        .saturating_sub(icon_w as usize)
        .saturating_sub(1)
        .max(1);
    let truncated = fit_label(label, label_max);
    let mut spans = vec![Span::styled(prefix, prefix_style)];
    let idxs = scorer.char_indices(&truncated);
    let ranges = fuzzy::char_indices_to_byte_ranges(&truncated, &idxs);
    if ranges.is_empty() {
        spans.push(Span::styled(truncated, base));
    } else {
        let mut cursor = 0usize;
        for (s, e) in ranges {
            if s > cursor {
                spans.push(Span::styled(truncated[cursor..s].to_string(), base));
            }
            if e > s {
                spans.push(Span::styled(truncated[s..e].to_string(), match_style));
            }
            cursor = e;
        }
        if cursor < truncated.len() {
            spans.push(Span::styled(truncated[cursor..].to_string(), base));
        }
    }
    // padding to push the icon flush right, then the icon span.
    let used: usize = spans.iter().map(|sp| sp.content.chars().count()).sum();
    let pad = (area_width as usize)
        .saturating_sub(used)
        .saturating_sub(icon_w as usize);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    if !icon_glyph.is_empty() {
        spans.push(Span::styled(icon_glyph.to_string(), action.icon_style()));
    }
    spans
}

/// Candidate list skin shared by field popup and Highlight history completion.
/// Selection / match colors and selected-row prefix come from [`theme`].
/// `checked` (same length as `labels`, or empty) marks Tab multi-select rows.
/// When `bordered` is true, draws a rounded popup shell (standalone field/history
/// popups); when false, fills `area` with no chrome (Picker left pane already
/// has an outer shell).
pub fn render_candidate_list(
    title: &str,
    labels: &[String],
    styles: &[Style],
    checked: &[bool],
    actions: &[ActionKind],
    selected: usize,
    empty_msg: &str,
    query: &str,
    frame: &mut Frame,
    area: Rect,
    bordered: bool,
) {
    let inner = if bordered {
        clear_to_canvas(frame, area);
        let block = popup_block(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        area
    };
    if labels.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                empty_msg,
                Style::default().add_modifier(Modifier::DIM),
            )),
            inner,
        );
        return;
    }
    let sel = selected.min(labels.len() - 1);
    // ViewportPaint: only build ListItems (+ fuzzy highlight) for visible rows.
    let n = labels.len();
    let (offset, end) = candidate_viewport_range(n, sel, inner.height as usize);
    // Reuse one FuzzyScorer across the viewport (same query × many labels).
    let mut paint_scorer = crate::fuzzy::FuzzyScorer::new(query);
    let items: Vec<ListItem> = (offset..end)
        .map(|i| {
            let label = &labels[i];
            let is_sel = i == sel;
            let is_checked = checked.get(i).copied().unwrap_or(false);
            let mut base = if is_sel {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            // Kind/field-colored candidates keep their fg when not selected.
            if !is_sel {
                if let Some(style) = styles.get(i) {
                    if let Some(fg) = style.fg {
                        base = base.fg(fg);
                    }
                }
            }
            ListItem::new(Line::from(candidate_label_spans_with_scorer(
                label,
                &mut paint_scorer,
                is_sel,
                is_checked,
                base,
                actions.get(i).copied().unwrap_or(ActionKind::None),
                inner.width,
            )))
            .style(base)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default())
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(sel.saturating_sub(offset)));
    frame.render_stateful_widget(list, inner, &mut state);
}

/// Visible `[offset, end)` window for a candidate list (ViewportPaint SLO).
pub fn candidate_viewport_range(n: usize, selected: usize, view_h: usize) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let view_h = view_h.max(1);
    let sel = selected.min(n - 1);
    let offset = sel
        .saturating_sub(view_h.saturating_sub(1) / 2)
        .min(n.saturating_sub(view_h));
    let end = (offset + view_h).min(n);
    (offset, end)
}

/// Candidate popup height: `clamp(count,1,8)+2` for border, clamped to
/// space below the modal anchor (Input / Search / H7 msg share this).
pub fn candidate_popup_rect(anchor: Rect, frame: Rect, match_count: usize) -> Rect {
    let desired = match_count.clamp(1, 8) as u16 + 2;
    stack_below_rect_gapped(anchor, frame, desired)
}

/// H1 Preview window: fill remaining space below the previous stack item
/// (candidates or modal), leaving [`POPUP_GAP`] when possible.
pub fn preview_popup_rect(anchor: Rect, frame: Rect) -> Rect {
    let flush_y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space_flush = frame_bottom.saturating_sub(flush_y);
    let height = if space_flush > POPUP_GAP {
        space_flush.saturating_sub(POPUP_GAP)
    } else {
        space_flush
    };
    stack_below_rect_gapped(anchor, frame, height)
}

/// Content rows inside a bordered Preview shell (`height - 2` for borders).
pub fn preview_content_capacity(area: Rect) -> usize {
    area.height.saturating_sub(2) as usize
}

/// Content rows available in the picker's right Preview pane for `frame`.
pub fn picker_preview_capacity(frame: Rect, left_ratio: f32) -> usize {
    let picker = picker_frame_rect(frame, true);
    let (_left, right) = split_picker_lr_gapped(picker, left_ratio);
    preview_content_capacity(right)
}

/// Content columns inside the picker's right Preview shell (`width - 2` for borders).
pub fn picker_preview_inner_width(frame: Rect, left_ratio: f32) -> u16 {
    let picker = picker_frame_rect(frame, true);
    let (_left, right) = split_picker_lr_gapped(picker, left_ratio);
    right.width.saturating_sub(2)
}

/// Search modal outer height: draft row + borders (candidates float below).
pub fn search_modal_height() -> u16 {
    3
}

/// Greedy word-wrap: returns byte ranges into `text`, one per physical
/// line, breaking on whitespace where possible. A single word longer than
/// `width` is hard-cut into `width`-sized pieces (never infinite-loops).
/// Leading/trailing whitespace around words is dropped; interior spacing
/// between words on the same line is preserved verbatim.
fn wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    if text.is_empty() {
        return vec![(0, 0)];
    }

    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut word_start: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = word_start.take() {
                words.push((s, i));
            }
        } else if word_start.is_none() {
            word_start = Some(i);
        }
    }
    if let Some(s) = word_start {
        words.push((s, text.len()));
    }
    if words.is_empty() {
        return vec![(0, text.len())];
    }

    let mut ranges = Vec::new();
    let mut line_start = words[0].0;
    let mut line_end = words[0].0;

    for &(ws, we) in &words {
        let word_chars = text[ws..we].chars().count();
        if word_chars > width {
            if line_end > line_start {
                ranges.push((line_start, line_end));
            }
            let char_offsets: Vec<usize> = text[ws..we]
                .char_indices()
                .map(|(i, _)| ws + i)
                .chain(std::iter::once(we))
                .collect();
            let mut c = 0usize;
            while c < char_offsets.len() - 1 {
                let take = width.min(char_offsets.len() - 1 - c);
                ranges.push((char_offsets[c], char_offsets[c + take]));
                c += take;
            }
            line_start = we;
            line_end = we;
            continue;
        }

        if line_end == line_start {
            line_end = we; // first word on an empty line always fits (checked above)
            continue;
        }
        let candidate_chars = text[line_start..we].chars().count();
        if candidate_chars > width {
            ranges.push((line_start, line_end));
            line_start = ws;
            line_end = we;
        } else {
            line_end = we;
        }
    }
    if line_end > line_start || ranges.is_empty() {
        ranges.push((line_start, line_end.max(line_start)));
    }
    ranges
}

/// Match segment: start, end, progressive color index, globally-active underline.
type ColoredMatch = (usize, usize, usize, bool);

/// Paint pattern: fuzzy pattern, color index, whether this is the globally active search.
type PaintPattern<'a> = (&'a str, usize, bool);

/// Merge a new interval into non-overlapping `result` (later pattern wins on overlap).
fn merge_colored_match(
    result: &mut Vec<ColoredMatch>,
    ns: usize,
    ne: usize,
    color_idx: usize,
    is_active: bool,
) {
    if ns >= ne {
        return;
    }
    let mut tmp = Vec::with_capacity(result.len() + 2);
    for (es, ee, ec, ea) in result.drain(..) {
        if ee <= ns || es >= ne {
            tmp.push((es, ee, ec, ea));
        } else {
            if es < ns {
                tmp.push((es, ns, ec, ea));
            }
            if ee > ne {
                tmp.push((ne, ee, ec, ea));
            }
        }
    }
    tmp.push((ns, ne, color_idx, is_active));
    tmp.sort_unstable_by_key(|&(s, _, _, _)| s);
    *result = tmp;
}

/// Collect substring paint ranges for one field (tag or msg); later patterns overwrite.
fn collect_field_matches(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    field: PaintField,
) -> Vec<ColoredMatch> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let mut result: Vec<ColoredMatch> = Vec::new();
    for &(pattern, color_idx, is_active) in patterns {
        let spans = crate::fuzzy::map_search_positions(&row.tag, &row.msg, &row.raw, pattern);
        for sp in spans {
            let ok = match field {
                PaintField::Tag => sp.field == PaintField::Tag,
                PaintField::Msg => sp.field == PaintField::Msg || sp.field == PaintField::Raw,
                PaintField::Raw => sp.field == PaintField::Raw,
            };
            if ok {
                merge_colored_match(&mut result, sp.start, sp.end, color_idx, is_active);
            }
        }
    }
    result
}

/// Splits `text[range.0..range.1]` into plain/highlighted spans.
/// Non-matched segments use `base`.
fn spans_for_range(
    text: &str,
    range: (usize, usize),
    matches: &[ColoredMatch],
    base: Style,
) -> Vec<Span<'static>> {
    let (start, end) = range;
    let mut spans = Vec::new();
    let mut cursor = start;
    for &(m_start, m_end, color_idx, is_active) in matches {
        if m_end <= start || m_start >= end {
            continue;
        }
        let seg_start = m_start.max(start);
        let seg_end = m_end.min(end);
        if seg_start > cursor {
            spans.push(Span::styled(text[cursor..seg_start].to_string(), base));
        }
        let style = if is_active {
            theme::highlight_style_active(color_idx)
        } else {
            theme::highlight_style(color_idx)
        };
        spans.push(Span::styled(text[seg_start..seg_end].to_string(), style));
        cursor = seg_end;
    }
    if cursor < end {
        spans.push(Span::styled(text[cursor..end].to_string(), base));
    }
    spans
}

/// Choose tag column width: prefer `preferred_max`, shrink on narrow panes so
/// the message still gets at least 8 columns.
fn tag_col_for_area_max(
    area_width: usize,
    prefix_without_tag: usize,
    preferred_max: usize,
) -> usize {
    let reserved = prefix_without_tag + TAG_MSG_GAP + 8;
    let available = area_width.saturating_sub(reserved);
    if available == 0 {
        return 0;
    }
    preferred_max.min(available).max(TAG_COL_MIN.min(available))
}

/// Choose tag column width: prefer [`TAG_COL_WIDTH`], shrink on narrow panes so
/// the message still gets at least 8 columns.
fn tag_col_for_area(area_width: usize, prefix_without_tag: usize) -> usize {
    tag_col_for_area_max(area_width, prefix_without_tag, TAG_COL_WIDTH)
}

/// Fit `tag` into a fixed display-column width: right-pad with spaces, or
/// truncate with `…`. Returns `(display, visible_byte_end)` where
/// `visible_byte_end` is the end of the prefix of `tag` shown before `…`
/// (equals `tag.len()` when not truncated).
fn fit_tag_column(tag: &str, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let tag_w = UnicodeWidthStr::width(tag);
    if tag_w <= width {
        let mut out = tag.to_string();
        out.push_str(&" ".repeat(width - tag_w));
        return (out, tag.len());
    }
    if width == 1 {
        return ("…".to_string(), 0);
    }
    let mut out = String::new();
    let mut used = 0usize;
    let mut byte_end = 0usize;
    for (i, ch) in tag.char_indices() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > width - 1 {
            break;
        }
        out.push(ch);
        used += cw;
        byte_end = i + ch.len_utf8();
    }
    out.push('…');
    used += 1;
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    (out, byte_end)
}

/// Append tag-column spans (highlights on the visible prefix only) + trailing pad.
fn push_tag_column_spans(
    spans: &mut Vec<Span<'static>>,
    tag: &str,
    tag_col: usize,
    tag_matches: &[ColoredMatch],
    tag_style: Style,
) {
    if tag_col == 0 {
        return;
    }
    let (fitted, visible_end) = fit_tag_column(tag, tag_col);
    let truncated = visible_end < tag.len();
    if tag_matches.is_empty() || visible_end == 0 {
        spans.push(Span::styled(fitted, tag_style));
        return;
    }
    spans.extend(spans_for_range(
        tag,
        (0, visible_end),
        tag_matches,
        tag_style,
    ));
    let mut used = UnicodeWidthStr::width(&tag[..visible_end]);
    if truncated {
        spans.push(Span::styled("…", tag_style));
        used += 1;
    }
    if used < tag_col {
        spans.push(Span::styled(" ".repeat(tag_col - used), tag_style));
    }
}

/// Tag + message styles: accent/bold tag and default msg, or theme-red for
/// severe (E/F/crash) rows. Fatal and crash signatures are bold.
fn entry_text_styles(row: &EntryRow) -> (Style, Style) {
    if !row.severe {
        return (
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
            Style::default(),
        );
    }
    let emphatic = !matches!(row.level, alnav::parser::Level::E);
    let base = theme::severe_entry_style(emphatic);
    (base.add_modifier(Modifier::BOLD), base)
}

fn render_entry_lines(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
    lineno: usize,
    lineno_width: usize,
) -> Vec<Line<'static>> {
    render_entry_lines_ex(
        row,
        patterns,
        area_width,
        Some((lineno, lineno_width)),
        false,
    )
}

fn render_compare_entry_lines(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
) -> Vec<Line<'static>> {
    render_entry_lines_ex(row, patterns, area_width, None, true)
}

/// Renders one log entry as one or more physical `Line`s: a header
/// (optional lineno / timestamp / optional pid·tid / level / fixed tag column)
/// followed by the message, word-wrapped to `area_width`.
fn render_entry_lines_ex(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
    lineno: Option<(usize, usize)>,
    show_pid_tid: bool,
) -> Vec<Line<'static>> {
    let lineno_s = match lineno {
        Some((n, w)) => format!("{n:>w$} "),
        None => String::new(),
    };
    let ts = format!("{} ", row.timestamp);
    let pid_s = if show_pid_tid && !row.pid.is_empty() {
        format!("{} ", row.pid)
    } else {
        String::new()
    };
    let tid_s = if show_pid_tid && !row.tid.is_empty() {
        format!("{} ", row.tid)
    } else {
        String::new()
    };
    let level_badge = format!(" {} ", row.level.as_char());
    let prefix_without_tag = lineno_s.chars().count()
        + ts.chars().count()
        + pid_s.chars().count()
        + tid_s.chars().count()
        + level_badge.chars().count()
        + LEVEL_TAG_GAP;
    let tag_col = tag_col_for_area(area_width, prefix_without_tag);
    let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
    let cont_prefix: String = " ".repeat(header_width);

    let first_width = area_width.saturating_sub(header_width).max(8);
    let cont_width = area_width.saturating_sub(header_width).max(8);

    let (tag_style, msg_style) = entry_text_styles(row);
    let tag_matches = collect_field_matches(row, patterns, PaintField::Tag);
    let msg_matches = collect_field_matches(row, patterns, PaintField::Msg);

    let first_pass = wrap_ranges(&row.msg, first_width);
    let mut line_ranges: Vec<(usize, usize)> = vec![first_pass[0]];
    let first_end = first_pass[0].1;
    if first_end < row.msg.len() {
        for (s, e) in wrap_ranges(&row.msg[first_end..], cont_width) {
            line_ranges.push((first_end + s, first_end + e));
        }
    }

    line_ranges
        .into_iter()
        .enumerate()
        .map(|(i, range)| {
            let mut spans = Vec::new();
            if i == 0 {
                if !lineno_s.is_empty() {
                    spans.push(Span::styled(
                        lineno_s.clone(),
                        theme::muted().add_modifier(Modifier::DIM),
                    ));
                }
                spans.push(Span::styled(ts.clone(), theme::muted()));
                if !pid_s.is_empty() {
                    spans.push(Span::styled(pid_s.clone(), theme::muted()));
                }
                if !tid_s.is_empty() {
                    spans.push(Span::styled(tid_s.clone(), theme::muted()));
                }
                spans.push(Span::styled(
                    level_badge.clone(),
                    theme::level_badge_style(row.level),
                ));
                spans.push(Span::styled(" ".repeat(LEVEL_TAG_GAP), Style::default()));
                push_tag_column_spans(&mut spans, &row.tag, tag_col, &tag_matches, tag_style);
                spans.push(Span::styled(" ".repeat(TAG_MSG_GAP), Style::default()));
            } else {
                spans.push(Span::styled(
                    cont_prefix.clone(),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            spans.extend(spans_for_range(&row.msg, range, &msg_matches, msg_style));
            Line::from(spans)
        })
        .collect()
}

/// Byte end of the first `max_chars` chars of `text` (or `text.len()` if shorter).
fn byte_end_for_chars(text: &str, max_chars: usize) -> usize {
    text.char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// Compact single-line Preview entry: timestamp/level/tag/msg colors like
/// LogList, but no lineno and a narrower tag column so msg keeps width.
fn render_entry_line_single(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
) -> Line<'static> {
    let ts = format!("{} ", row.timestamp);
    let level_badge = format!(" {} ", row.level.as_char());
    let prefix_without_tag = ts.chars().count() + level_badge.chars().count() + LEVEL_TAG_GAP;
    let tag_col = tag_col_for_area_max(area_width, prefix_without_tag, PREVIEW_TAG_COL_MAX);
    let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
    let msg_budget = area_width.saturating_sub(header_width).max(1);

    let (tag_style, msg_style) = entry_text_styles(row);
    let tag_matches = collect_field_matches(row, patterns, PaintField::Tag);
    let msg_matches = collect_field_matches(row, patterns, PaintField::Msg);

    let mut spans = Vec::new();
    spans.push(Span::styled(ts, theme::muted()));
    spans.push(Span::styled(
        level_badge,
        theme::level_badge_style(row.level),
    ));
    spans.push(Span::styled(" ".repeat(LEVEL_TAG_GAP), Style::default()));
    push_tag_column_spans(&mut spans, &row.tag, tag_col, &tag_matches, tag_style);
    spans.push(Span::styled(" ".repeat(TAG_MSG_GAP), Style::default()));

    let msg_chars = row.msg.chars().count();
    if msg_budget == 0 {
        return Line::from(spans);
    }
    if msg_chars <= msg_budget {
        spans.extend(spans_for_range(
            &row.msg,
            (0, row.msg.len()),
            &msg_matches,
            msg_style,
        ));
    } else if msg_budget == 1 {
        spans.push(Span::styled("…".to_string(), msg_style));
    } else {
        let visible_end = byte_end_for_chars(&row.msg, msg_budget - 1);
        spans.extend(spans_for_range(
            &row.msg,
            (0, visible_end),
            &msg_matches,
            msg_style,
        ));
        spans.push(Span::styled("…".to_string(), msg_style));
    }
    Line::from(spans)
}

/// Collapsed (single-line) LogList entry: same header layout as
/// `render_entry_lines` (lineno/timestamp/level/fixed tag column), but the
/// message is truncated with `…` instead of word-wrapped across multiple
/// `Line`s. Used when `App.collapsed_view` is toggled on (`w`).
fn render_entry_line_collapsed(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
    lineno: usize,
    lineno_width: usize,
) -> Line<'static> {
    let lineno_s = format!("{lineno:>lineno_width$} ");
    let ts = format!("{} ", row.timestamp);
    let level_badge = format!(" {} ", row.level.as_char());
    let prefix_without_tag =
        lineno_s.chars().count() + ts.chars().count() + level_badge.chars().count() + LEVEL_TAG_GAP;
    let tag_col = tag_col_for_area(area_width, prefix_without_tag);
    let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
    let msg_budget = area_width.saturating_sub(header_width).max(1);

    let (tag_style, msg_style) = entry_text_styles(row);
    let tag_matches = collect_field_matches(row, patterns, PaintField::Tag);
    let msg_matches = collect_field_matches(row, patterns, PaintField::Msg);

    let mut spans = Vec::new();
    spans.push(Span::styled(
        lineno_s,
        theme::muted().add_modifier(Modifier::DIM),
    ));
    spans.push(Span::styled(ts, theme::muted()));
    spans.push(Span::styled(
        level_badge,
        theme::level_badge_style(row.level),
    ));
    spans.push(Span::styled(" ".repeat(LEVEL_TAG_GAP), Style::default()));
    push_tag_column_spans(&mut spans, &row.tag, tag_col, &tag_matches, tag_style);
    spans.push(Span::styled(" ".repeat(TAG_MSG_GAP), Style::default()));

    let msg_chars = row.msg.chars().count();
    if msg_chars <= msg_budget {
        spans.extend(spans_for_range(
            &row.msg,
            (0, row.msg.len()),
            &msg_matches,
            msg_style,
        ));
    } else if msg_budget == 1 {
        spans.push(Span::styled("…".to_string(), msg_style));
    } else {
        let visible_end = byte_end_for_chars(&row.msg, msg_budget - 1);
        spans.extend(spans_for_range(
            &row.msg,
            (0, visible_end),
            &msg_matches,
            msg_style,
        ));
        spans.push(Span::styled("…".to_string(), msg_style));
    }
    Line::from(spans)
}

/// H3 minimap cell priority (higher wins on overlap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MinimapMark {
    Track = 0,
    Viewport = 1,
    Highlight = 2,
    Bookmark = 3,
    Severe = 4,
}

/// Max visible indices scanned per frame for search/severe marks (H3).
const MINIMAP_MARK_BUDGET: usize = 4000;

/// Map a `visible` index into a rail row (`height` cells).
pub fn minimap_row_for_index(index: usize, visible_len: usize, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    if visible_len <= 1 || height == 1 {
        return 0;
    }
    index.saturating_mul(height - 1) / (visible_len - 1)
}

/// Build H3 rail marks for `height` cells. Empty when `visible` is empty.
pub fn build_minimap_marks(app: &App, height: u16) -> Vec<MinimapMark> {
    let h = height as usize;
    if h == 0 || app.visible.is_empty() {
        return Vec::new();
    }
    let n = app.visible.len();
    let mut cells = vec![MinimapMark::Track; h];

    // Approximate viewport band from list_offset (item units ≈ 1 row each).
    let start = app.list_offset.min(n.saturating_sub(1));
    let vp_items = h.max(1).min(n);
    let end = (start + vp_items).min(n).max(start + 1);
    let v0 = minimap_row_for_index(start, n, h);
    let v1 = minimap_row_for_index(end - 1, n, h);
    for r in v0..=v1 {
        if cells[r] < MinimapMark::Viewport {
            cells[r] = MinimapMark::Viewport;
        }
    }

    // File: never O(budget) `row_at` — severe via prefetch cache, highlight via
    // async hit index. Stream keeps owned-row sample parse (bounded buffer).
    if app.store.is_file() {
        let samples = n.min(MINIMAP_MARK_BUDGET);
        for s in 0..samples {
            let i = if samples <= 1 {
                0
            } else {
                s * (n - 1) / (samples - 1)
            };
            let Some(src) = app.source_idx_for_visible(i) else {
                continue;
            };
            if app.store.as_file().and_then(|f| f.severe_cached(src)) == Some(true) {
                let r = minimap_row_for_index(i, n, h);
                cells[r] = MinimapMark::Severe;
            }
        }
        let hits = &app.highlight_scan.hits;
        let hit_n = hits.len();
        if hit_n > 0 {
            let take = hit_n.min(MINIMAP_MARK_BUDGET);
            for s in 0..take {
                let hi = if take <= 1 {
                    0
                } else {
                    s * (hit_n - 1) / (take - 1)
                };
                let i = hits[hi];
                if i >= n {
                    continue;
                }
                let r = minimap_row_for_index(i, n, h);
                if cells[r] < MinimapMark::Highlight {
                    cells[r] = MinimapMark::Highlight;
                }
            }
        }
    } else {
        let samples = n.min(MINIMAP_MARK_BUDGET);
        for s in 0..samples {
            let i = if samples <= 1 {
                0
            } else {
                s * (n - 1) / (samples - 1)
            };
            let Some(row) = app.row_at(i) else {
                continue;
            };
            let r = minimap_row_for_index(i, n, h);
            if app.highlight_groups.any_match(&row.tag, &row.msg)
                && cells[r] < MinimapMark::Highlight
            {
                cells[r] = MinimapMark::Highlight;
            }
            if row.severe {
                cells[r] = MinimapMark::Severe;
            }
        }
    }

    // Bookmarks (F5): O(bookmarks) via row_id→visible lookup — never scan /
    // parse all visible rows (FileStore would O(n) parse multi-million files).
    if !app.bookmarks.items.is_empty() {
        for bm in &app.bookmarks.items {
            if !app.bookmark_alive(bm.row_id()) {
                continue;
            }
            if let Some(i) = app.visible_idx_for_row_id(bm.row_id()) {
                let r = minimap_row_for_index(i, n, h);
                if cells[r] < MinimapMark::Bookmark {
                    cells[r] = MinimapMark::Bookmark;
                }
            }
        }
    }
    cells
}

fn render_minimap(app: &App, frame: &mut Frame, rail: Rect) {
    if rail.width == 0 || rail.height == 0 {
        return;
    }
    let marks = build_minimap_marks(app, rail.height);
    if marks.is_empty() {
        return;
    }
    let buf = frame.buffer_mut();
    for (dy, mark) in marks.iter().enumerate() {
        let y = rail.y.saturating_add(dy as u16);
        if y >= rail.y.saturating_add(rail.height) {
            break;
        }
        let cell = &mut buf[(rail.x, y)];
        match mark {
            MinimapMark::Track => {
                cell.set_char('│');
                cell.set_style(theme::minimap_track_style());
            }
            MinimapMark::Viewport => {
                cell.set_char('│');
                cell.set_style(theme::minimap_viewport_style());
            }
            MinimapMark::Highlight => {
                cell.set_char('•');
                cell.set_style(theme::minimap_highlight_style());
            }
            MinimapMark::Bookmark => {
                cell.set_char('•');
                cell.set_style(Style::default().fg(theme::bookmark_minimap_color()));
            }
            MinimapMark::Severe => {
                cell.set_char('•');
                cell.set_style(theme::minimap_severe_style());
            }
        }
    }
}

/// Takes `&mut App` (unlike sibling `render_*` functions) so ratatui's
/// scroll offset can be persisted across frames via `App.list_offset` —
/// do not revert this to `&App`, that's exactly what caused the old
/// viewport-snap bug.
pub fn render_log_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let active = app.focus == Focus::LogList;
    let loading = app.log_loading_label();
    let title = theme::numbered_title_with_loading(4, "Log", active, loading.as_deref());
    let block = rounded_block(title, active);
    let inner = block.inner(area);
    // H3: reserve 1 inner column for the minimap when there is content.
    let rail_w = if !app.visible.is_empty() && inner.width > 1 {
        1u16
    } else {
        0
    };
    let content_w = inner.width.saturating_sub(rail_w).max(1);
    let inner_width = content_w as usize;
    let selection = app.selection_range();
    let patterns = app.highlight_groups.paint_patterns(app.active_highlight);

    let lineno_width = app.visible.len().max(1).to_string().len();

    // Compute list_area before building items for the virtual-scroll window size.
    // block.inner() is a pure rect computation — it does not render anything.
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: content_w,
        height: inner.height,
    };
    // M2: one-line compare-tray summary at top of Log (folded when empty).
    let bm_lines = if app.bookmarks.is_empty() || content_area.height <= 1 {
        Vec::new()
    } else {
        vec![bookmark_summary_line(&app.bookmarks.summary_line())]
    };
    let bm_h = bm_lines.len() as u16;
    let (bm_area_opt, list_area) = if bm_h > 0 && content_area.height > bm_h {
        let [top, rest] =
            Layout::vertical([Constraint::Length(bm_h), Constraint::Fill(1)]).areas(content_area);
        (Some((top, bm_lines)), rest)
    } else {
        (None, content_area)
    };

    // ── Virtual scroll ─────────────────────────────────────────────────────────
    // Build ListItems only for a window around the current scroll position.
    // 3× the viewport height provides a safe margin for multi-line entries.
    let n = app.visible.len();
    let viewport_h = (list_area.height as usize).max(1);
    let window_size = (viewport_h * 3).max(20);

    // Align window so the cursor is always inside it:
    //  • cursor above list_offset  → slide window up to cursor (smooth k scrolling)
    //  • cursor past window bottom → anchor at cursor − viewport_h (G / follow append)
    //  • otherwise                 → keep window at list_offset
    let window_start = if n == 0 {
        0
    } else if app.cursor < app.list_offset {
        app.cursor
    } else if app.cursor >= app.list_offset.saturating_add(window_size) {
        app.cursor.saturating_sub(viewport_h)
    } else {
        app.list_offset
    };
    let window_start = window_start.min(n.saturating_sub(1));
    let window_end = (window_start + window_size).min(n);

    // cursor position relative to the window (always in-bounds after alignment).
    let rel_cursor = if n > 0 {
        app.cursor.saturating_sub(window_start)
    } else {
        0
    };

    let items: Vec<ListItem> = if n == 0 {
        Vec::new()
    } else {
        (window_start..window_end)
            .filter_map(|abs_i| {
                let row = app.row_at(abs_i)?;
                let lines = if app.collapsed_view {
                    vec![render_entry_line_collapsed(
                        &row,
                        &patterns,
                        inner_width,
                        abs_i + 1,
                        lineno_width,
                    )]
                } else {
                    render_entry_lines(&row, &patterns, inner_width, abs_i + 1, lineno_width)
                };
                let mut item = ListItem::new(lines);
                if let Some((lo, hi)) = selection {
                    if abs_i >= lo && abs_i <= hi {
                        item = item.style(theme::log_visual_style());
                    } else if app.is_bookmark_row(row.row_id) {
                        item = item.style(theme::bookmark_row_style());
                    }
                } else if app.is_bookmark_row(row.row_id) {
                    item = item.style(theme::bookmark_row_style());
                } else if active && abs_i == app.cursor {
                    item = item.style(theme::log_selection_style());
                }
                Some(item)
            })
            .collect()
    };

    // Paint border first; list fills the content columns only (no block).
    frame.render_widget(block, area);
    if let Some((area, lines)) = bm_area_opt {
        render_bookmark_strip(frame, area, lines);
    }

    // rel_offset is always 0: window_start == list_offset in the stable case,
    // so the relative offset within the window is 0. ratatui computes the final
    // scroll position (state.offset()) to keep rel_cursor visible, and we store
    // the absolute result back into app.list_offset below.
    let list = List::new(items);
    let mut state = ListState::default().with_offset(0);
    if n > 0 {
        state.select(Some(rel_cursor));
    }
    frame.render_stateful_widget(list, list_area, &mut state);
    // Restore absolute offset: window_start + what ratatui settled on.
    app.list_offset = window_start + state.offset();

    if rail_w > 0 && inner.height > 0 {
        let rail = Rect {
            x: inner.x.saturating_add(content_w),
            y: inner.y,
            width: 1,
            height: inner.height,
        };
        render_minimap(app, frame, rail);
    }
}

/// One-line Log-top compare-tray summary.
fn bookmark_summary_line(text: &str) -> Line<'static> {
    let painted = text.replacen('★', theme::GLYPH_BOOKMARK_PIN, 1);
    Line::from(Span::styled(
        format!(" {painted}"),
        theme::bookmark_label_style(),
    ))
}

/// M2: one summary line inside the Log region.
pub fn render_bookmark_strip(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(Block::default().style(theme::bookmark_strip_style()), area);
    let shown: Vec<Line<'static>> = lines.into_iter().take(area.height as usize).collect();
    frame.render_widget(Paragraph::new(shown), area);
}

fn group_dot_span(enabled: bool, selected: bool) -> Span<'static> {
    let dot = if enabled {
        theme::GLYPH_GROUP_ON
    } else {
        theme::GLYPH_GROUP_OFF
    };
    // Selection uses the same Magenta accent as region selection frames;
    // kept to one cell so the strip can stay a single content row tall.
    let style = if selected {
        theme::chip_group_border_style(true).add_modifier(Modifier::BOLD)
    } else if !enabled {
        theme::disabled_chip_style()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Span::styled(dot.to_string(), style)
}

fn filter_group_spans(g: &Group, selected: bool) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(g.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    if g.chips.is_empty() {
        let style = if !g.enabled {
            theme::disabled_chip_style()
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {} ", g.label), style));
    } else {
        for (i, chip) in g.chips.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" ".repeat(PILL_GAP as usize)));
            }
            let pill = theme::chip_pill_spans(chip.field, &chip.value, !g.enabled);
            spans.extend(pill);
        }
    }
    spans
}

fn highlight_group_spans(
    g: &HighlightGroup,
    color_idx: usize,
    selected: bool,
    active_global: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(g.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let pill = theme::highlight_pill_spans(&g.pattern, color_idx, !g.enabled, active_global);
    spans.extend(pill);
    spans
}

fn exclude_entry_spans(
    e: &crate::filter_model::ExcludeEntry,
    selected: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(e.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let pill = theme::exclude_pill_spans(e.chip.field, &e.chip.value, !e.enabled);
    spans.extend(pill);
    spans
}

fn span_width(span: &Span<'_>) -> usize {
    span.content.chars().count()
}

fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for span in spans {
        let w = span_width(&span);
        if w > width {
            if !current.is_empty() {
                lines.push(Line::from(std::mem::take(&mut current)));
                used = 0;
            }
            let text = span.content.as_ref().to_string();
            let style = span.style;
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let end = (i + width).min(chars.len());
                let chunk: String = chars[i..end].iter().collect();
                lines.push(Line::from(Span::styled(chunk, style)));
                i = end;
            }
            continue;
        }
        if !current.is_empty() && used + w > width {
            lines.push(Line::from(std::mem::take(&mut current)));
            used = 0;
        }
        used += w;
        current.push(span);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(Line::from(current));
    }
    lines
}

fn flow_wrap_groups(groups: Vec<Vec<Span<'static>>>, width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut row_spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for group in groups {
        let group_w: usize = group.iter().map(span_width).sum();
        if group_w > width {
            if !row_spans.is_empty() {
                out.push(Line::from(std::mem::take(&mut row_spans)));
                used = 0;
            }
            out.extend(wrap_spans(group, width));
            continue;
        }
        let need = if row_spans.is_empty() {
            group_w
        } else {
            CHIP_GROUP_GAP as usize + group_w
        };
        if !row_spans.is_empty() && used + need > width {
            out.push(Line::from(std::mem::take(&mut row_spans)));
            used = 0;
        }
        if !row_spans.is_empty() {
            row_spans.push(Span::raw(" ".repeat(CHIP_GROUP_GAP as usize)));
            used += CHIP_GROUP_GAP as usize;
        }
        used += group_w;
        row_spans.extend(group);
    }
    if !row_spans.is_empty() {
        out.push(Line::from(row_spans));
    }
    out
}

fn filter_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::ChipStrip;
    let groups: Vec<Vec<Span<'static>>> = app
        .groups
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| filter_group_spans(g, i == app.group_cursor && active))
        .collect();
    flow_wrap_groups(groups, inner_width)
}

fn highlight_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::HighlightStrip;
    let mut color_idx = 0usize;
    let groups: Vec<Vec<Span<'static>>> = app
        .highlight_groups
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let idx = if g.enabled {
                let c = color_idx;
                color_idx += 1;
                c
            } else {
                0
            };
            highlight_group_spans(
                g,
                idx,
                i == app.highlight_cursor && active,
                Some(i) == app.active_highlight,
            )
        })
        .collect();
    flow_wrap_groups(groups, inner_width)
}

fn exclude_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::ExcludeStrip;
    let groups: Vec<Vec<Span<'static>>> = app
        .groups
        .excludes
        .iter()
        .enumerate()
        .map(|(i, e)| exclude_entry_spans(e, i == app.exclude_cursor && active))
        .collect();
    flow_wrap_groups(groups, inner_width)
}

/// Strip height: `0` when empty, else `2` (rounded region chrome) + content
/// rows. Content is a single terminal row per wrap line — nested per-chip
/// `Block`s need 3 rows each and made the strip ~2× taller than a cell's
/// visual proportions allow.
pub fn filter_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.groups.groups.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = filter_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

/// Same rules as [`filter_strip_height`] for the Exclude strip (H9).
pub fn exclude_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.groups.excludes.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = exclude_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

/// Same rules as [`filter_strip_height`] for the Search strip.
pub fn highlight_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.highlight_groups.groups.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = highlight_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

pub fn render_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ChipStrip;
    let block = divider_block(theme::numbered_title(1, "Filter", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(filter_strip_lines(app, inner.width)), inner);
}

pub fn render_exclude_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ExcludeStrip;
    let block = divider_block(theme::numbered_title(2, "Exclude", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(exclude_strip_lines(app, inner.width)), inner);
}

pub fn render_highlight_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::HighlightStrip;
    let block = divider_block(theme::numbered_title(3, "Highlight", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(highlight_strip_lines(app, inner.width)),
        inner,
    );
}

fn committed_chip_spans(chips: &[crate::input::Chip], exclude_mode: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, chip) in chips.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(PILL_GAP as usize)));
        }
        let pill = if exclude_mode {
            theme::exclude_pill_spans(chip.field, &chip.value, false)
        } else {
            theme::chip_pill_spans(chip.field, &chip.value, false)
        };
        spans.extend(pill);
    }
    spans
}

/// Wrap committed pills to `width` (each chip is an atomic flow group).
fn committed_chip_lines(
    chips: &[crate::input::Chip],
    exclude_mode: bool,
    width: u16,
) -> Vec<Line<'static>> {
    if chips.is_empty() {
        return Vec::new();
    }
    let groups: Vec<Vec<Span<'static>>> = chips
        .iter()
        .map(|chip| {
            if exclude_mode {
                theme::exclude_pill_spans(chip.field, &chip.value, false)
            } else {
                theme::chip_pill_spans(chip.field, &chip.value, false)
            }
        })
        .collect();
    flow_wrap_groups(groups, width)
}

/// Rows needed for committed chips in the picker search band.
/// Caps so at least one candidate row remains above the search input.
fn committed_chip_rows(
    chips: &[crate::input::Chip],
    exclude_mode: bool,
    width: u16,
    left_height: u16,
) -> u16 {
    if chips.is_empty() {
        return 0;
    }
    let rows = committed_chip_lines(chips, exclude_mode, width)
        .len()
        .max(1) as u16;
    let max_chip = left_height.saturating_sub(PICKER_SEARCH_HEIGHT.saturating_add(1));
    rows.min(max_chip)
}

/// Display-column width of styled spans (sum of content widths).
fn spans_display_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// Window `text` around `caret` (char index) so the caret stays visible within
/// `max_cols` display columns. Hardware cursor needs no reserved glyph column.
fn window_around_caret(text: &str, caret: usize, max_cols: usize) -> (String, String) {
    let caret = caret.min(text.chars().count());
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars[..caret].iter().collect();
    let after: String = chars[caret..].iter().collect();
    if max_cols == 0 {
        return (String::new(), String::new());
    }
    let bw = before.width();
    let aw = after.width();
    if bw + aw <= max_cols {
        return (before, after);
    }
    if bw <= max_cols {
        let room = max_cols - bw;
        let mut out_after = String::new();
        let mut w = 0;
        for ch in after.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > room {
                break;
            }
            out_after.push(ch);
            w += cw;
        }
        return (before, out_after);
    }
    // before too long: keep a suffix ending at the caret.
    let mut w = 0;
    let mut start = caret;
    while start > 0 {
        let ch = chars[start - 1];
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max_cols {
            break;
        }
        w += cw;
        start -= 1;
    }
    let before_win: String = chars[start..caret].iter().collect();
    let room = max_cols.saturating_sub(w);
    let mut out_after = String::new();
    let mut aw = 0;
    for ch in chars[caret..].iter().copied() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if aw + cw > room {
            break;
        }
        out_after.push(ch);
        aw += cw;
    }
    (before_win, out_after)
}

/// Windowed draft text for hardware-cursor editing.
///
/// Returns `(spans, caret_col)` where `caret_col` is the display-column offset
/// of the caret within the returned draft spans (end of the visible `before`).
pub fn editable_text_spans(
    text: &str,
    caret: usize,
    max_width: Option<u16>,
) -> (Vec<Span<'static>>, u16) {
    let caret = caret.min(text.chars().count());
    let text_budget = max_width.map(|w| w as usize);
    let (before, after) = match text_budget {
        Some(budget) => window_around_caret(text, caret, budget),
        None => {
            let byte = text
                .char_indices()
                .nth(caret)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            (text[..byte].to_string(), text[byte..].to_string())
        }
    };
    let caret_col = before.width().min(usize::from(u16::MAX)) as u16;
    let mut spans = Vec::new();
    if !before.is_empty() {
        spans.push(Span::styled(before, Style::reset()));
    }
    if !after.is_empty() {
        spans.push(Span::styled(after, Style::reset()));
    }
    (spans, caret_col)
}

/// Build Input draft line spans; returns caret column within `inner` when editing.
fn input_content_spans(
    input: &InputBox,
    show_caret: bool,
    max_width: Option<u16>,
) -> (Vec<Span<'static>>, Option<u16>) {
    let mut spans = committed_chip_spans(&input.chips, input.exclude_mode);
    // Gap + reset after pills so draft never sits inside the pill fill.
    if !input.chips.is_empty() {
        spans.push(Span::styled(" ".repeat(PILL_GAP as usize), Style::reset()));
    }
    if let Some(field) = input.draft_field {
        spans.push(Span::styled(
            format!("{} {}:", theme::field_icon(field), field.keyword()),
            Style::reset().fg(theme::field_color(field)),
        ));
    }
    if show_caret {
        let prefix_w = spans_display_width(&spans);
        let draft_max = max_width.map(|w| (w as usize).saturating_sub(prefix_w) as u16);
        let (draft_spans, caret_col) =
            editable_text_spans(input.draft.as_str(), input.draft.cursor(), draft_max);
        spans.extend(draft_spans);
        let col = (prefix_w as u16).saturating_add(caret_col);
        (spans, Some(col))
    } else {
        spans.push(Span::styled(input.draft.to_string(), Style::reset()));
        (spans, None)
    }
}

/// Centered Input modal (visible while `Focus::Input`).
/// Returns hardware cursor position when in Insert mode.
pub fn render_input_modal(
    input: &InputBox,
    mode: Mode,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let title = if input.exclude_mode {
        "Input !"
    } else {
        "Input"
    };
    let inner = render_modal_shell(title, frame, area);
    let (spans, caret_col) = input_content_spans(input, mode == Mode::Insert, Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    caret_col.map(|col| Position {
        // `col == width` means after the last visible char (into right padding/border).
        x: inner.x.saturating_add(col.min(inner.width)),
        y: inner.y,
    })
}

/// Legacy single-row Input render kept for unit tests that draw into a fixed area.
pub fn render_input_box(
    input: &InputBox,
    mode: Mode,
    focused: bool,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let block = divider_block(theme::numbered_title(5, "Input", focused), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    clear_to_canvas(frame, inner);
    let (spans, caret_col) = input_content_spans(input, mode == Mode::Insert, Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    caret_col.map(|col| Position {
        x: inner.x.saturating_add(col.min(inner.width)),
        y: inner.y,
    })
}

/// Centered Highlight modal: draft row only (history candidates float below).
/// Returns hardware cursor position for the draft caret.
pub fn render_highlight_modal(
    search: &HighlightBox,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let inner = render_modal_shell("Highlight", frame, area);
    let (spans, caret_col) = editable_text_spans(
        search.draft.as_str(),
        search.draft.cursor(),
        Some(inner.width),
    );
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    Some(Position {
        x: inner.x.saturating_add(caret_col.min(inner.width)),
        y: inner.y,
    })
}

/// Outer height for H4 Detail modal (border + content), clamped to `frame`.
pub fn detail_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let desired = (content_rows as u16).saturating_add(2).max(3);
    let max = frame.height.saturating_mul(3) / 5;
    let max = max.max(5).min(frame.height.saturating_sub(1));
    desired.min(max).max(3)
}

/// Horizontal Unicode-block bar (`█`/`░`), proportional to `count / max`.
/// Shared by the summary panel's level-distribution and Top-tags sections
/// (Top errors intentionally has no bar — see `render_summary_panel`).
fn bar_line(label: &str, count: usize, max: usize, width: usize, color: Style) -> Line<'static> {
    let ratio = if max == 0 {
        0.0
    } else {
        count as f64 / max as f64
    };
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let empty = width.saturating_sub(filled);
    Line::from(vec![
        Span::styled(
            pad_display(label, SUMMARY_LABEL_WIDTH),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(" "),
        Span::styled("█".repeat(filled), color),
        Span::styled(
            "░".repeat(empty),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(format!(" {count}")),
    ])
}

const SUMMARY_BAR_WIDTH: usize = 20;
const SUMMARY_LABEL_WIDTH: usize = 12;

/// Pad/truncate `s` to a fixed display width (Unicode-width aware).
fn pad_display(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    while w < width {
        out.push(' ');
        w += 1;
    }
    out
}

/// Body lines for the Ready summary panel (used by render + height calc).
/// Aligned with CLI `--summary` fields: total rows, time range, crashes,
/// level distribution (bar), Top 10 tags (bar), Top 10 errors (no bar).
fn summary_report_lines(report: &alnav::summary::SummaryOutput) -> Vec<Line<'static>> {
    use alnav::parser::Level;

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::raw(format!("Rows: {}", report.total))));
    let first = if report.time_range.first.is_empty() {
        "-"
    } else {
        report.time_range.first.as_str()
    };
    let last = if report.time_range.last.is_empty() {
        "-"
    } else {
        report.time_range.last.as_str()
    };
    lines.push(Line::from(Span::raw(format!(
        "Time range: {first} — {last}"
    ))));
    lines.push(Line::from(Span::raw(format!(
        "Crashes: {}",
        report.crashes
    ))));
    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        "Levels",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    let level_order = [Level::V, Level::D, Level::I, Level::W, Level::E, Level::F];
    let level_max = level_order
        .iter()
        .map(|l| *report.levels.get(&l.as_char()).unwrap_or(&0))
        .max()
        .unwrap_or(0);
    for level in level_order {
        let count = *report.levels.get(&level.as_char()).unwrap_or(&0);
        if count == 0 {
            continue;
        }
        lines.push(bar_line(
            &level.as_char().to_string(),
            count,
            level_max,
            SUMMARY_BAR_WIDTH,
            theme::level_bar_style(level),
        ));
    }
    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        "Top tags",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    if report.top_tags.is_empty() {
        lines.push(Line::from(Span::styled(
            "(none)",
            theme::preview_placeholder_style(),
        )));
    }
    let tag_max = report.top_tags.iter().map(|t| t.count).max().unwrap_or(0);
    for entry in &report.top_tags {
        lines.push(bar_line(
            &entry.tag,
            entry.count,
            tag_max,
            SUMMARY_BAR_WIDTH,
            theme::accent_bar_style(),
        ));
        let mut parts: Vec<String> = entry
            .levels
            .iter()
            .map(|(c, n)| format!("{c}:{n}"))
            .collect();
        parts.sort();
        if !parts.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("    {}", parts.join(" ")),
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
    }
    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        "Top errors",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    if report.top_errors.is_empty() {
        lines.push(Line::from(Span::styled(
            "(none)",
            theme::preview_placeholder_style(),
        )));
    }
    for (i, err) in report.top_errors.iter().enumerate() {
        lines.push(Line::from(Span::raw(format!(
            "{:>2}. [{}] {} ({})",
            i + 1,
            err.tag,
            err.pattern,
            err.count
        ))));
        lines.push(Line::from(Span::styled(
            format!("    {}", err.sample),
            Style::default().add_modifier(Modifier::DIM),
        )));
    }
    lines
}

/// Row count for `Loading` (1, the placeholder line) or `Ready` content —
/// used by `main.rs` to size the modal before `render_summary_panel` runs.
pub fn summary_content_row_count(app: &App) -> usize {
    use crate::app::SummaryView;
    match &app.summary_view {
        SummaryView::Closed => 0,
        SummaryView::Loading => 1,
        SummaryView::Ready(report) => summary_report_lines(report).len(),
    }
    .max(1)
}

/// Height for the summary panel modal given frame size and content.
pub fn summary_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let max = frame.height.saturating_sub(4).max(8);
    let want = (content_rows as u16).saturating_add(2); // border
    want.min(max).max(8)
}

const HIST_PREVIEW_ROWS: usize = 6;
const HIST_BAR_WIDTH: usize = 16;

pub fn hist_content_row_count(app: &App) -> usize {
    use crate::hist_panel::HistView;
    match &app.hist_view {
        HistView::Closed => 0,
        HistView::Loading { .. } => 1,
        HistView::Ready(report) => report.buckets.len().saturating_add(HIST_PREVIEW_ROWS + 2),
    }
    .max(1)
}

pub fn hist_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let max = frame.height.saturating_sub(4).max(10);
    let want = (content_rows as u16).saturating_add(2);
    want.min(max).max(10)
}

fn hist_bucket_line(
    bucket: &crate::hist_panel::HistBucket,
    selected: bool,
    spike: bool,
) -> Line<'static> {
    use alnav::parser::Level;
    let total = bucket.total().max(1);
    let width = HIST_BAR_WIDTH;
    let segs = [
        (bucket.v, Level::V),
        (bucket.d, Level::D),
        (bucket.i, Level::I),
        (bucket.w, Level::W),
        (bucket.e, Level::E),
        (bucket.f, Level::F),
    ];
    let mut filled = 0usize;
    let mut spans = Vec::new();
    let label_style = if selected {
        theme::candidate_selected_style()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    let mark = if selected { "› " } else { "  " };
    spans.push(Span::styled(format!("{mark}{} ", bucket.key), label_style));
    for (count, level) in segs {
        let n = ((count as f64 / total as f64) * width as f64).round() as usize;
        let n = n.min(width.saturating_sub(filled));
        if n > 0 {
            spans.push(Span::styled("█".repeat(n), theme::level_bar_style(level)));
            filled += n;
        }
    }
    if filled < width {
        spans.push(Span::styled(
            "░".repeat(width - filled),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    let mut tail = format!(" {}", bucket.total());
    if bucket.severe > 0 {
        tail.push_str(&format!(" E{}", bucket.severe));
    }
    if spike {
        tail.push_str(" !");
    }
    spans.push(Span::styled(
        tail,
        if selected {
            theme::candidate_selected_style()
        } else if bucket.severe > 0 {
            theme::severe_entry_style(false)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        },
    ));
    Line::from(spans)
}

pub fn render_hist_panel(app: &App, frame: &mut Frame, area: Rect) {
    use crate::hist_panel::HistView;
    if area.height == 0 {
        return;
    }
    let title = match &app.hist_view {
        HistView::Ready(r) => {
            format!(
                "Hist {}",
                alnav::histogram::format_interval(r.interval_secs)
            )
        }
        HistView::Loading { interval_secs } => {
            format!(
                "Hist {} ",
                alnav::histogram::format_interval(*interval_secs)
            )
        }
        HistView::Closed => "Hist".into(),
    };
    let inner = render_modal_shell(&title, frame, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    match &app.hist_view {
        HistView::Closed => {}
        HistView::Loading { .. } => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "computing…",
                    theme::log_loading_style(true),
                ))),
                inner,
            );
        }
        HistView::Ready(report) => {
            let preview_h = HIST_PREVIEW_ROWS.min(inner.height.saturating_sub(3) as usize);
            let chunks =
                Layout::vertical([Constraint::Min(1), Constraint::Length(preview_h as u16)])
                    .split(inner);
            let list_h = chunks[0].height as usize;
            let start = app
                .hist_cursor
                .saturating_sub(list_h.saturating_sub(1) / 2)
                .min(report.buckets.len().saturating_sub(list_h));
            let lines: Vec<Line<'static>> = report
                .buckets
                .iter()
                .enumerate()
                .skip(start)
                .take(list_h)
                .map(|(i, b)| hist_bucket_line(b, i == app.hist_cursor, report.is_spike(&b.key)))
                .collect();
            frame.render_widget(Paragraph::new(lines), chunks[0]);
            if let Some(bucket) = report.buckets.get(app.hist_cursor) {
                let mut preview = Vec::new();
                preview.push(Line::from(Span::styled(
                    "preview",
                    Style::default().add_modifier(Modifier::DIM),
                )));
                for row in bucket.preview.iter().take(preview_h.saturating_sub(1)) {
                    preview.push(Line::from(Span::styled(
                        format!("{} {} {}", row.timestamp, row.tag, row.msg),
                        theme::muted(),
                    )));
                }
                if preview.len() == 1 {
                    preview.push(Line::from(Span::styled(
                        "no rows",
                        theme::preview_placeholder_style(),
                    )));
                }
                frame.render_widget(Paragraph::new(preview), chunks[1]);
            }
        }
    }
}

/// Leader `i` summary panel: `Loading` placeholder or `Ready` stats body.
/// `Esc` closes without resuming follow (`app.close_summary_panel`); content
/// is a static snapshot — it never refreshes while open (see PRD R1).
pub fn render_summary_panel(app: &App, frame: &mut Frame, area: Rect) {
    use crate::app::SummaryView;
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell("Summary", frame, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    match &app.summary_view {
        SummaryView::Closed => {}
        SummaryView::Loading => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "computing…",
                    theme::log_loading_style(true),
                ))),
                inner,
            );
        }
        SummaryView::Ready(report) => {
            let lines = summary_report_lines(report);
            let scroll = app.summary_scroll.min(lines.len().saturating_sub(1));
            let visible: Vec<Line<'static>> = lines.into_iter().skip(scroll).collect();
            frame.render_widget(
                Paragraph::new(visible).wrap(ratatui::widgets::Wrap { trim: false }),
                inner,
            );
        }
    }
}

/// Build H4 Fields-mode lines for the current row (used by render + height).
pub fn detail_field_lines(
    row: Option<&crate::model::EntryRow>,
    inner_width: u16,
) -> Vec<Line<'static>> {
    use crate::input::ChipField;

    let label_w = 5usize;
    let value_w = inner_width.saturating_sub(label_w as u16 + 1).max(1) as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(row) = row else {
        lines.push(Line::from(Span::styled(
            "no row".to_string(),
            theme::preview_placeholder_style(),
        )));
        return lines;
    };

    let push_kv = |lines: &mut Vec<Line<'static>>,
                   label: &str,
                   label_style: Style,
                   value: String,
                   value_style: Style| {
        let label_pad = format!("{label:<width$}", width = label_w);
        let mut first = true;
        for (s, e) in wrap_ranges(&value, value_w) {
            let chunk = value[s..e].to_string();
            if first {
                lines.push(Line::from(vec![
                    Span::styled(label_pad.clone(), label_style),
                    Span::raw(" "),
                    Span::styled(chunk, value_style),
                ]));
                first = false;
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(label_w + 1)),
                    Span::styled(chunk, value_style),
                ]));
            }
        }
        if first {
            lines.push(Line::from(vec![
                Span::styled(label_pad, label_style),
                Span::raw(" "),
            ]));
        }
    };

    push_kv(
        &mut lines,
        "time",
        theme::detail_label_style(),
        row.timestamp.clone(),
        theme::muted(),
    );
    {
        let level_ch = row.level.as_char().to_string();
        let label_pad = format!("{:<width$}", "level", width = label_w);
        lines.push(Line::from(vec![
            Span::styled(label_pad, theme::detail_field_label_style(ChipField::Level)),
            Span::raw(" "),
            Span::styled(format!(" {level_ch} "), theme::level_badge_style(row.level)),
        ]));
    }
    push_kv(
        &mut lines,
        ChipField::Pid.keyword(),
        theme::detail_field_label_style(ChipField::Pid),
        row.pid.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Tid.keyword(),
        theme::detail_field_label_style(ChipField::Tid),
        row.tid.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Tag.keyword(),
        theme::detail_field_label_style(ChipField::Tag),
        row.tag.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Pkg.keyword(),
        theme::detail_field_label_style(ChipField::Pkg),
        row.pkg.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Msg.keyword(),
        theme::detail_field_label_style(ChipField::Msg),
        row.msg.clone(),
        Style::default(),
    );
    lines
}

/// Try JSON pretty-print: `msg` first, then `raw`. Returns `(text, is_json)`.
pub fn pretty_json_for_row(row: &crate::model::EntryRow) -> (String, bool) {
    for candidate in [&row.msg, &row.raw] {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate.trim()) {
            if let Ok(pretty) = serde_json::to_string_pretty(&value) {
                return (pretty, true);
            }
        }
    }
    (row.msg.clone(), false)
}

/// H5 Pretty-mode lines (used by render + height).
pub fn detail_pretty_lines(
    row: Option<&crate::model::EntryRow>,
    inner_width: u16,
) -> Vec<Line<'static>> {
    let width = inner_width.max(1) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(row) = row else {
        lines.push(Line::from(Span::styled(
            "no row".to_string(),
            theme::preview_placeholder_style(),
        )));
        return lines;
    };
    let (text, is_json) = pretty_json_for_row(row);
    if !is_json {
        lines.push(Line::from(Span::styled(
            "not JSON".to_string(),
            theme::preview_placeholder_style(),
        )));
    }
    for (s, e) in wrap_ranges(&text, width) {
        lines.push(Line::from(Span::raw(text[s..e].to_string())));
    }
    lines
}

fn crash_detector() -> &'static CrashDetector {
    static DETECTOR: OnceLock<CrashDetector> = OnceLock::new();
    DETECTOR.get_or_init(CrashDetector::new)
}

/// Continuation-line scan cap for File-mode crash stack merging (R2).
const CRASH_SCAN_LIMIT: usize = 500;

/// Structured crash/ANR info for the cursor row's msg, when it matches a
/// crash signature. `None` lets the caller fall back to the existing
/// JSON/raw Pretty chain (R1). The `bool` flags a File-mode continuation
/// scan that hit [`CRASH_SCAN_LIMIT`] before finding the next parsed row.
pub fn crash_context_for_row(app: &App, row: &EntryRow) -> Option<(CrashInfo, bool)> {
    let crash_type = crash_detector().detect(&row.as_log_entry())?;

    let (merged_msg, truncated) = if app.store.is_file() {
        let mut merged = row.msg.clone();
        let mut truncated = false;
        if let Some(start) = app.source_idx_for_visible(app.cursor) {
            let mut idx = start + 1;
            let mut scanned = 0usize;
            loop {
                if scanned >= CRASH_SCAN_LIMIT {
                    truncated = true;
                    break;
                }
                let Some(next) = app.store.row_at_source(idx, false) else {
                    break;
                };
                if next.parsed {
                    break;
                }
                merged.push('\n');
                merged.push_str(&next.msg);
                idx += 1;
                scanned += 1;
            }
        }
        (merged, truncated)
    } else {
        (row.msg.clone(), false)
    };

    let entry = LogEntry {
        timestamp: &row.timestamp,
        pid: &row.pid,
        tid: &row.tid,
        level: row.level,
        tag: &row.tag,
        pkg: &row.pkg,
        msg: &merged_msg,
    };
    Some((crash_detector().parse_crash(&entry, crash_type), truncated))
}

/// H_crash structured detail lines (R4). `is_stream` picks the empty-stack
/// placeholder copy; `truncated` appends the 500-line scan-cap notice.
pub fn render_crash_detail_lines(
    info: &CrashInfo,
    is_stream: bool,
    truncated: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();

    let type_label = match info.crash_type {
        CrashType::FatalException => "FATAL EXCEPTION",
        CrashType::Anr => "ANR",
        CrashType::NativeCrash => "NATIVE CRASH",
    };
    let badge_style = Style::default()
        .fg(theme::warning())
        .add_modifier(Modifier::BOLD);
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", theme::GLYPH_CRASH), badge_style),
        Span::styled(type_label.to_string(), badge_style),
    ]));
    for (s, e) in wrap_ranges(&info.headline, width) {
        lines.push(Line::from(Span::raw(info.headline[s..e].to_string())));
    }
    if let Some(exception) = &info.exception {
        for (s, e) in wrap_ranges(exception, width) {
            lines.push(Line::from(Span::styled(
                exception[s..e].to_string(),
                Style::default().fg(theme::warning()),
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        format!(
            "pid={} tid={} tag={} time={}",
            info.pid, info.tid, info.tag, info.timestamp
        ),
        theme::muted(),
    )));
    if info.stack.is_empty() {
        let placeholder = if is_stream {
            "no stack (stream)"
        } else {
            "no stack"
        };
        lines.push(Line::from(Span::styled(
            placeholder.to_string(),
            theme::preview_placeholder_style(),
        )));
    } else {
        for frame in &info.stack {
            for (s, e) in wrap_ranges(frame, width) {
                lines.push(Line::from(Span::raw(frame[s..e].to_string())));
            }
        }
    }
    if truncated {
        lines.push(Line::from(Span::styled(
            "…(truncated)".to_string(),
            theme::preview_placeholder_style(),
        )));
    }
    lines
}

/// Content lines for the current detail mode (height estimation + render).
pub fn detail_content_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    use crate::app::DetailView;
    match app.detail {
        DetailView::Fields => detail_field_lines(app.current_row().as_deref(), inner_width),
        DetailView::Pretty => {
            if let Some(row) = app.current_row() {
                if let Some((info, truncated)) = crash_context_for_row(app, &row) {
                    let width = inner_width.max(1) as usize;
                    return render_crash_detail_lines(
                        &info,
                        !app.store.is_file(),
                        truncated,
                        width,
                    );
                }
            }
            detail_pretty_lines(app.current_row().as_deref(), inner_width)
        }
        DetailView::Closed => Vec::new(),
    }
}

/// H4/H5 row-detail overlay.
pub fn render_detail(app: &App, frame: &mut Frame, area: Rect) {
    use crate::app::DetailView;
    if matches!(app.detail, DetailView::Closed) || area.height == 0 {
        return;
    }
    let title = match app.detail {
        DetailView::Fields => "Detail",
        DetailView::Pretty => "Pretty",
        DetailView::Closed => return,
    };
    let inner = render_modal_shell(title, frame, area);
    let lines = detail_content_lines(app, inner.width);
    let max_rows = inner.height as usize;
    let shown: Vec<Line<'static>> = lines.into_iter().take(max_rows).collect();
    frame.render_widget(Paragraph::new(shown), inner);
}

/// Outer height for the global time-window panel (`tt`).
pub fn time_panel_height(frame: Rect) -> u16 {
    // border(2) + 4 field rows + section labels(2) + up to 5 candidate rows
    let desired = 13u16;
    let max = frame.height.saturating_mul(3) / 5;
    let max = max.max(8).min(frame.height.saturating_sub(1));
    desired.min(max).max(8)
}

/// Render `tt` time panel. Returns hardware cursor for the focused field.
pub fn render_time_panel(app: &App, frame: &mut Frame, area: Rect) -> Option<Position> {
    use crate::time_panel::TimeField;

    let panel = app.time_panel.as_ref()?;
    if area.height == 0 {
        return None;
    }
    let inner = render_modal_shell("Time", frame, area);
    if inner.height == 0 || inner.width == 0 {
        return None;
    }

    let focus = panel.focus;
    let cand_budget = inner
        .height
        .saturating_sub(6) // 2 labels + 4 field rows
        .min(5) as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut caret_row: u16 = 0;
    let mut caret_col: u16 = 0;
    let mut row: u16 = 0;

    let push_label = |lines: &mut Vec<Line<'static>>, text: &str, row: &mut u16| {
        lines.push(Line::from(Span::styled(
            text.to_string(),
            Style::default().add_modifier(Modifier::DIM),
        )));
        *row = row.saturating_add(1);
    };

    let push_field = |lines: &mut Vec<Line<'static>>,
                      label: &str,
                      value: &str,
                      caret: usize,
                      active: bool,
                      row: &mut u16,
                      caret_row: &mut u16,
                      caret_col: &mut u16,
                      inner_w: u16| {
        let prefix = format!("{label} ");
        let prefix_w = UnicodeWidthStr::width(prefix.as_str()) as u16;
        let style = if active {
            Style::default().fg(theme::accent())
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let budget = inner_w.saturating_sub(prefix_w).max(1);
        let (value_spans, col) = editable_text_spans(value, caret, Some(budget));
        let mut spans = vec![Span::styled(prefix, style)];
        spans.extend(value_spans);
        lines.push(Line::from(spans));
        if active {
            *caret_row = *row;
            *caret_col = prefix_w.saturating_add(col);
        }
        *row = row.saturating_add(1);
    };

    push_label(&mut lines, "since", &mut row);
    push_field(
        &mut lines,
        "date",
        panel.since_date_query(),
        panel.since_date_cursor(),
        focus == TimeField::SinceDate,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );
    if focus == TimeField::SinceDate {
        let filtered = panel.filtered_dates(true);
        let hl = panel.since_date_highlight();
        for (i, stats) in filtered.into_iter().take(cand_budget).enumerate() {
            let selected = hl == Some(i);
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker} {}", stats.date),
                style,
            )));
            row = row.saturating_add(1);
        }
    }
    push_field(
        &mut lines,
        "time",
        panel.since_time(),
        panel.since_time_cursor(),
        focus == TimeField::SinceTime,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );

    push_label(&mut lines, "until", &mut row);
    push_field(
        &mut lines,
        "date",
        panel.until_date_query(),
        panel.until_date_cursor(),
        focus == TimeField::UntilDate,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );
    if focus == TimeField::UntilDate {
        let filtered = panel.filtered_dates(false);
        let hl = panel.until_date_highlight();
        for (i, stats) in filtered.into_iter().take(cand_budget).enumerate() {
            let selected = hl == Some(i);
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker} {}", stats.date),
                style,
            )));
            row = row.saturating_add(1);
        }
    }
    push_field(
        &mut lines,
        "time",
        panel.until_time(),
        panel.until_time_cursor(),
        focus == TimeField::UntilTime,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );

    let max_rows = inner.height as usize;
    lines.truncate(max_rows);
    frame.render_widget(Paragraph::new(lines), inner);

    let y = inner
        .y
        .saturating_add(caret_row.min(inner.height.saturating_sub(1)));
    // `caret_col == width` means after the last visible char.
    let x = inner.x.saturating_add(caret_col.min(inner.width));
    Some(Position { x, y })
}

/// H1 Preview window: LogList-styled single-line hits (Filter / Highlight / Search).
pub fn render_preview(
    title: &str,
    lines: &[crate::preview::PreviewHit],
    placeholder: &str,
    frame: &mut Frame,
    area: Rect,
) {
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell(title, frame, area);
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                placeholder.to_string(),
                theme::preview_placeholder_style(),
            )),
            inner,
        );
        return;
    }
    let area_width = inner.width as usize;
    let items: Vec<ListItem> = lines
        .iter()
        .map(|hit| {
            let patterns: Vec<PaintPattern<'_>> = hit
                .pattern
                .as_deref()
                .map(|p| vec![(p, 0usize, true)])
                .unwrap_or_default();
            ListItem::new(render_entry_line_single(&hit.row, &patterns, area_width))
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

/// fzf left-pane committed pills plus search prompt:
/// mode icon (search / `＋` / `✎`) + optional `field:` + draft.
/// Returns hardware cursor position for the draft caret.
///
/// `chip_rows` is the height reserved above the rounded input for wrapped
/// committed chips (must match [`picker_left_stack`]).
pub fn render_picker_search_line(
    mode: &crate::picker::PickerMode,
    text: &str,
    caret: usize,
    chips: &[crate::input::Chip],
    exclude_chips: bool,
    draft_field: Option<ChipField>,
    // Override mode icon (e.g. bookmark glyph for Bookmark picker).
    prompt_icon: Option<&'static str>,
    chip_rows: u16,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    if area.height == 0 {
        return None;
    }

    let chip_h = chip_rows.min(
        area.height
            .saturating_sub(PICKER_SEARCH_HEIGHT.min(area.height)),
    );
    if chip_h > 0 && !chips.is_empty() {
        let chip_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: chip_h,
        };
        let lines = committed_chip_lines(chips, exclude_chips, chip_area.width);
        frame.render_widget(Paragraph::new(lines), chip_area);
    }

    let input_area = Rect {
        x: area.x,
        y: area.y.saturating_add(chip_h),
        width: area.width,
        height: area.height.saturating_sub(chip_h),
    };
    if input_area.height == 0 {
        return None;
    }

    let block = rounded_block(Line::from(""), true);
    let inner = block.inner(input_area);
    frame.render_widget(block, input_area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let padding = PICKER_SEARCH_HORIZONTAL_PADDING.min(inner.width / 2);
    let content = Rect {
        x: inner.x.saturating_add(padding),
        y: inner.y,
        width: inner.width.saturating_sub(padding.saturating_mul(2)),
        height: inner.height,
    };
    if content.width == 0 {
        return None;
    }

    let mut prompt_spans = vec![match prompt_icon {
        Some(icon) => theme::picker_prompt_prefix(icon),
        None => theme::picker_mode_prefix(mode),
    }];
    if let Some(field) = draft_field {
        prompt_spans.push(Span::styled(
            format!("{} {}:", theme::field_icon(field), field.keyword()),
            Style::reset().fg(theme::field_color(field)),
        ));
    }
    let prefix_w = spans_display_width(&prompt_spans);
    let draft_max = (content.width as usize).saturating_sub(prefix_w) as u16;
    let (draft_spans, caret_col) = editable_text_spans(text, caret, Some(draft_max));
    prompt_spans.extend(draft_spans);
    frame.render_widget(Paragraph::new(Line::from(prompt_spans)), content);
    let x_off = (prefix_w as u16).saturating_add(caret_col);
    // `x_off == content.width` places the caret after the last visible char
    // (into the right horizontal padding).
    Some(Position {
        x: content.x.saturating_add(x_off.min(content.width)),
        y: content.y,
    })
}

/// Right-pane content for [`render_picker`].
pub enum PickerRightPane<'a> {
    /// Filter/Highlight-style sampled log hits.
    Hits(&'a [crate::preview::PreviewHit]),
    /// Preset panel: chip-strip style Filter → Exclude → Highlight.
    ChipRules(&'a [Line<'static>]),
    /// Open-file head preview (plain text lines).
    PlainLines(&'a [Line<'static>]),
}

/// fzf-style picker shell: left candidates + bottom search, right Preview.
/// Returns hardware cursor position for the search-line caret.
pub fn render_picker(
    title: &str,
    mode: &crate::picker::PickerMode,
    search_text: &str,
    caret: usize,
    match_query: &str,
    chips: &[crate::input::Chip],
    exclude_chips: bool,
    draft_field: Option<ChipField>,
    labels: &[String],
    styles: &[Style],
    checked: &[bool],
    actions: &[ActionKind],
    selected: usize,
    empty_msg: &str,
    right_pane: PickerRightPane<'_>,
    left_ratio: f32,
    show_preview: bool,
    prompt_icon: Option<&'static str>,
    frame: &mut Frame,
    frame_area: Rect,
) -> Option<Position> {
    let picker_area = picker_frame_rect(frame_area, show_preview);
    clear_to_canvas(frame, picker_area);

    // Honor caller `left_ratio` (config `picker_left_ratio`) for both preview and
    // compact pickers. Preview used to hard-code 0.3, which crushed Open-file labels.
    let split_ratio = left_ratio;
    let (left, right) = if show_preview {
        let (l, r) = split_picker_lr_gapped(picker_area, split_ratio);
        (l, Some(r))
    } else {
        (picker_area, None)
    };
    let left_inner = render_modal_shell(title, frame, left);
    let chip_rows = committed_chip_rows(chips, exclude_chips, left_inner.width, left_inner.height);
    let (candidates_area, search_area) = picker_left_stack(left_inner, chip_rows);

    if candidates_area.height > 0 {
        render_candidate_list(
            "list",
            labels,
            styles,
            checked,
            actions,
            selected,
            empty_msg,
            match_query,
            frame,
            candidates_area,
            false,
        );
    }
    let cursor = render_picker_search_line(
        mode,
        search_text,
        caret,
        chips,
        exclude_chips,
        draft_field,
        prompt_icon,
        chip_rows,
        frame,
        search_area,
    );
    if let Some(right) = right {
        match right_pane {
            PickerRightPane::Hits(preview_lines) => {
                render_preview("Preview", preview_lines, "no preview", frame, right);
            }
            PickerRightPane::ChipRules(lines) => {
                render_preset_rules_preview(lines, frame, right);
            }
            PickerRightPane::PlainLines(lines) => {
                render_plain_preview(lines, frame, right);
            }
        }
    }
    cursor
}

fn render_plain_preview(lines: &[Line<'static>], frame: &mut Frame, area: Rect) {
    let inner = render_modal_shell("Path", frame, area);
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "no selection",
                theme::preview_placeholder_style(),
            )),
            inner,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(lines.to_vec()).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// Preset Preview: stacked Filter / Exclude / Highlight chip rows (strip style).
pub fn render_preset_rules_preview(lines: &[Line<'static>], frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell("Preview", frame, area);
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("no rules", theme::preview_placeholder_style())),
            inner,
        );
        return;
    }
    frame.render_widget(Paragraph::new(lines.to_vec()), inner);
}

/// Build Preview lines for a preset (Filter → Exclude → Highlight), strip-like pills.
pub fn preset_preview_lines(preset: &crate::preset::Preset, width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut out = Vec::new();
    if let Ok((groups, highlights)) = crate::preset::materialize(preset) {
        if !groups.groups.is_empty() {
            out.push(Line::from(Span::styled(
                "Filter",
                theme::muted().add_modifier(Modifier::BOLD),
            )));
            let groups_spans: Vec<Vec<Span<'static>>> = groups
                .groups
                .iter()
                .map(|g| filter_group_spans(g, false))
                .collect();
            out.extend(flow_wrap_groups(groups_spans, width as u16));
        }
        if !groups.excludes.is_empty() {
            if !out.is_empty() {
                out.push(Line::from(""));
            }
            out.push(Line::from(Span::styled(
                "Exclude",
                theme::muted().add_modifier(Modifier::BOLD),
            )));
            let groups_spans: Vec<Vec<Span<'static>>> = groups
                .excludes
                .iter()
                .map(|e| exclude_entry_spans(e, false))
                .collect();
            out.extend(flow_wrap_groups(groups_spans, width as u16));
        }
        if !highlights.groups.is_empty() {
            if !out.is_empty() {
                out.push(Line::from(""));
            }
            out.push(Line::from(Span::styled(
                "Highlight",
                theme::muted().add_modifier(Modifier::BOLD),
            )));
            let mut color_idx = 0usize;
            let groups_spans: Vec<Vec<Span<'static>>> = highlights
                .groups
                .iter()
                .map(|g| {
                    let idx = if g.enabled {
                        let c = color_idx;
                        color_idx += 1;
                        c
                    } else {
                        0
                    };
                    highlight_group_spans(g, idx, false, false)
                })
                .collect();
            out.extend(flow_wrap_groups(groups_spans, width as u16));
        }
    }
    out
}

/// Save / rename preset name dialog (no candidates, no preview).
/// Returns hardware cursor position for the draft caret.
pub fn render_preset_name_dialog(
    dialog: &crate::preset::PresetNameDialog,
    frame: &mut Frame,
    frame_area: Rect,
) -> Option<Position> {
    use crate::preset::PresetNamePurpose;

    let title = match &dialog.purpose {
        PresetNamePurpose::Save => "Save preset",
        PresetNamePurpose::Rename { .. } => "Rename preset",
    };
    let modal_w = modal_width(frame_area.width).min(48);
    let area = centered_modal_rect(frame_area, modal_w, 5);
    clear_to_canvas(frame, area);
    let inner = render_modal_shell(title, frame, area);
    if dialog.confirm_overwrite {
        let text = vec![
            Line::from(Span::styled(
                format!("Overwrite '{}'?", dialog.field.as_str()),
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            Line::from(Span::styled(
                "y/Enter confirm  n/Esc cancel",
                theme::context_help_style(),
            ))
            .alignment(Alignment::Center),
        ];
        frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
        return None;
    }
    let mut spans = vec![theme::picker_mode_prefix(&crate::picker::PickerMode::New)];
    let prefix_w = spans_display_width(&spans) as u16;
    let draft_max = inner.width.saturating_sub(prefix_w);
    let (draft_spans, caret_col) = editable_text_spans(
        dialog.field.as_str(),
        dialog.field.cursor(),
        Some(draft_max),
    );
    spans.extend(draft_spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    Some(Position {
        x: inner
            .x
            .saturating_add(prefix_w.saturating_add(caret_col).min(inner.width)),
        y: inner.y,
    })
}

/// Bookmark picker right pane: same Fields shell/content as LogList `p`.
pub fn render_picker_detail(row: Option<&crate::model::EntryRow>, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell("Detail", frame, area);
    let lines = if row.is_some() {
        detail_field_lines(row, inner.width)
    } else {
        vec![Line::from(Span::styled(
            "row gone".to_string(),
            theme::preview_placeholder_style(),
        ))]
    };
    let max_rows = inner.height as usize;
    let shown: Vec<Line<'static>> = lines.into_iter().take(max_rows).collect();
    frame.render_widget(Paragraph::new(shown), inner);
}

/// Destructive confirm, overlaid at the frame center.
fn confirm_dialog_question(confirm: &crate::picker::ConfirmKind) -> String {
    match confirm {
        crate::picker::ConfirmKind::DeleteMany { items } => {
            if items.len() == 1 {
                "Delete selected?".to_string()
            } else {
                format!("Delete {} items?", items.len())
            }
        }
        crate::picker::ConfirmKind::DeletePreset { name } => format!("Delete preset '{name}'?"),
        crate::picker::ConfirmKind::ClearAll => "Clear all rules?".to_string(),
    }
}

pub fn render_confirm_dialog(
    confirm: &crate::picker::ConfirmKind,
    frame: &mut Frame,
    frame_area: Rect,
) {
    let question = confirm_dialog_question(confirm);
    let width = 34.min(frame_area.width).max(1);
    let height = 5.min(frame_area.height).max(1);
    let area = centered_modal_rect(frame_area, width, height);
    let inner = render_modal_shell("Confirm", frame, area);
    let text = vec![
        Line::from(Span::styled(
            question,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            "y/Enter confirm  n/Esc cancel",
            theme::context_help_style(),
        ))
        .alignment(Alignment::Center),
    ];
    frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
}

/// Search history-chip candidates — same shell as Input field popup.
pub fn render_highlight_popup(
    search: &HighlightBox,
    groups: &[HighlightGroup],
    frame: &mut Frame,
    area: Rect,
) {
    let candidates = search.candidate_indices(groups);
    let n = candidates.len().min(6);
    let labels: Vec<String> = candidates
        .iter()
        .take(n)
        .map(|&i| groups[i].pattern.clone())
        .collect();
    // Color by each group's global enabled-pattern index for consistency
    // with strip pills; fall back to dim if disabled.
    let mut color_idx = 0usize;
    let mut group_color: Vec<Option<usize>> = Vec::with_capacity(groups.len());
    for g in groups {
        if g.enabled {
            group_color.push(Some(color_idx));
            color_idx += 1;
        } else {
            group_color.push(None);
        }
    }
    let styles: Vec<Style> = candidates
        .iter()
        .take(n)
        .map(|&i| match group_color[i] {
            Some(idx) => theme::highlight_style(idx),
            None => theme::disabled_chip_style(),
        })
        .collect();
    let selected = if n == 0 {
        0
    } else {
        search.selected.min(n - 1)
    };
    render_candidate_list(
        "History",
        &labels,
        &styles,
        &[],
        &[],
        selected,
        "no history",
        &search.draft,
        frame,
        area,
        true,
    );
}

pub fn render_popup(input: &InputBox, frame: &mut Frame, area: Rect) {
    if !input.field_popup_visible() {
        return;
    }
    let matches = input.field_candidates();
    let labels: Vec<String> = matches.iter().map(|f| f.keyword().to_string()).collect();
    let styles: Vec<Style> = matches
        .iter()
        .map(|&f| Style::default().fg(theme::field_color(f)))
        .collect();
    let selected = if matches.is_empty() {
        0
    } else {
        input.field_selected.min(matches.len() - 1)
    };
    render_candidate_list(
        "Fields",
        &labels,
        &styles,
        &[],
        &[],
        selected,
        "no match",
        &input.draft,
        frame,
        area,
        true,
    );
}

const FLASH_MIN: usize = 12;

pub fn render_status_bar(app: &mut App, frame: &mut Frame, area: Rect) {
    let mut left = vec![Span::styled(
        format!("{}/{}", app.cursor + 1, app.visible.len()),
        Style::default().add_modifier(Modifier::DIM),
    )];
    if let Some((current, total)) = app.highlight_match_stats() {
        let k = current.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_SEARCH,
            &format!("{k}/{total}"),
            theme::accent(),
        ));
    }
    left.push(Span::raw(" "));
    if app.following {
        left.push(theme::status_pill(theme::GLYPH_FOLLOWING, theme::success()));
    } else {
        left.push(theme::status_icon_dim(theme::GLYPH_FOLLOWING));
    }
    // Source vs disconnect share one slot (live disconnect replaces source glyph).
    left.push(Span::raw(" "));
    if app.export_source.is_live() && app.ingest_done {
        left.push(theme::status_pill(
            theme::GLYPH_DISCONNECT,
            theme::warning(),
        ));
    } else {
        left.push(theme::status_pill(
            app.export_source.status_glyph(),
            theme::accent(),
        ));
    }
    if let Some(lock) = app.lock_badge_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_LOCK,
            &lock,
            theme::lock(),
        ));
    }
    if let Some(time) = app.time_badge_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_TIME,
            &time,
            theme::lock(),
        ));
    }
    if let Some(focus) = app.view_focus_badge_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_VIEW_FOCUS,
            focus,
            theme::accent(),
        ));
    }
    if let Some(prog) = app.file_progress_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_PROGRESS,
            &prog,
            theme::warning(),
        ));
    }
    if app.visual_anchor.is_some() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill(theme::GLYPH_VISUAL, theme::accent()));
    }

    let area_w = area.width as usize;
    let left_w: usize = left.iter().map(span_width).sum();
    let avail = area_w.saturating_sub(left_w);

    let mut flash_span: Option<Span<'static>> = None;
    let mut flash_block_w = 0usize;
    if let Some(msg) = app.status_msg.as_deref() {
        let full = theme::status_flash_pill(msg);
        let natural = 1 + span_width(&full);
        let slot = natural.max(FLASH_MIN);
        let hint_budget = avail.saturating_sub(slot).saturating_sub(1);
        if hint_budget < crate::help::MIN_HELP_WIDTH && natural > avail {
            let pill_max = avail.saturating_sub(1);
            if pill_max == 0 {
                flash_span = None;
                flash_block_w = 0;
            } else {
                let fitted = theme::status_flash_pill_fit(msg, pill_max);
                flash_block_w = 1 + span_width(&fitted);
                flash_span = Some(fitted);
            }
        } else {
            flash_span = Some(full);
            flash_block_w = natural;
        }
    }

    let hint_budget = if flash_span.is_some() {
        let slot = flash_block_w.max(FLASH_MIN);
        avail.saturating_sub(slot).saturating_sub(1)
    } else {
        avail.saturating_sub(1)
    };
    let hints = crate::help::context_hint_spans(app, hint_budget);
    let hint_w: usize = hints
        .as_ref()
        .map(|h| h.iter().map(span_width).sum())
        .unwrap_or(0);

    let mut spans = left;
    if let Some(flash) = flash_span {
        spans.push(Span::raw(" "));
        spans.push(flash);
    }
    if hints.is_some() {
        let pad = area_w.saturating_sub(left_w + flash_block_w + hint_w);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        if let Some(hint) = hints {
            spans.extend(hint);
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Command palette width: 60% of frame, clamped 40–72.
pub fn command_palette_width(frame: Rect) -> u16 {
    let sixty = (u32::from(frame.width) * 60 / 100) as u16;
    sixty.clamp(40, 72).min(frame.width).max(1)
}

/// VS Code-style command palette: input shell, optional candidate dropdown.
pub fn render_command_palette(app: &App, frame: &mut Frame, frame_area: Rect) -> Option<Position> {
    let Some(palette) = app.command_palette.as_ref() else {
        return None;
    };
    let width = command_palette_width(frame_area);
    let input_h = search_modal_height();
    let input_area = top_modal_rect(frame_area, width, input_h);
    let inner = render_modal_shell_glyph(
        theme::GLYPH_TITLE_PALETTE,
        "Command Palette",
        frame,
        input_area,
    );
    let query = palette.query.as_str();
    let (spans, caret_col) = editable_text_spans(query, palette.query.cursor(), Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    let cursor = Some(Position {
        x: inner
            .x
            .saturating_add(caret_col.min(inner.width.saturating_sub(1))),
        y: inner.y,
    });

    if query.is_empty() {
        return cursor;
    }

    let hits = crate::action::filtered_catalog(app, query);
    let list_h = if hits.is_empty() {
        3
    } else {
        (hits.len().min(crate::command_palette::PALETTE_VISIBLE_ROWS) as u16).saturating_add(2)
    };
    let list_area = stack_below_rect_gapped(input_area, frame_area, list_h.max(3));
    if list_area.height == 0 {
        return cursor;
    }
    clear_to_canvas(frame, list_area);
    let block = rounded_block(
        theme::plain_title(theme::GLYPH_TITLE_PALETTE, "", true),
        true,
    );
    let list_inner = block.inner(list_area);
    frame.render_widget(block, list_area);
    clear_to_canvas(frame, list_inner);
    if list_inner.width == 0 || list_inner.height == 0 {
        return cursor;
    }

    if hits.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No matching commands",
                theme::candidate_unselected_style(),
            )),
            list_inner,
        );
        return cursor;
    }

    let n = hits.len();
    let sel = palette.selected.min(n - 1);
    let view_h = list_inner.height as usize;
    let (offset, end) = candidate_viewport_range(n, sel, view_h);
    let mut scorer = crate::fuzzy::FuzzyScorer::new(query);
    let items: Vec<ListItem> = (offset..end)
        .map(|i| {
            let item = &hits[i];
            let is_sel = i == sel;
            let mut base = if is_sel {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            let line = palette_row_spans(item, &mut scorer, is_sel, list_inner.width, &mut base);
            ListItem::new(Line::from(line)).style(base)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default())
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(sel.saturating_sub(offset)));
    frame.render_stateful_widget(list, list_inner, &mut state);
    cursor
}

fn palette_row_spans(
    item: &crate::action::PaletteItem,
    scorer: &mut crate::fuzzy::FuzzyScorer,
    selected: bool,
    area_width: u16,
    base: &mut Style,
) -> Vec<Span<'static>> {
    use crate::bookmark::fit_label;
    let hint = &item.key_hint;
    let hint_w = UnicodeWidthStr::width(hint.as_str()) as u16;
    let icon_w: u16 = 2; // glyph + space
    let gap: u16 = if hint.is_empty() { 0 } else { 1 };
    let title_max = (area_width as usize)
        .saturating_sub(icon_w as usize)
        .saturating_sub(hint_w as usize)
        .saturating_sub(gap as usize)
        .max(1);
    let truncated = fit_label(item.title, title_max);
    let match_style = theme::candidate_match_style(selected);
    let idxs = scorer.char_indices(&truncated);
    let ranges = crate::fuzzy::char_indices_to_byte_ranges(&truncated, &idxs);
    let mut spans = vec![Span::styled(format!("{} ", item.icon), *base)];
    if ranges.is_empty() {
        spans.push(Span::styled(truncated.clone(), *base));
    } else {
        let mut cursor = 0usize;
        for (s, e) in ranges {
            if s > cursor {
                spans.push(Span::styled(truncated[cursor..s].to_string(), *base));
            }
            if e > s {
                spans.push(Span::styled(truncated[s..e].to_string(), match_style));
            }
            cursor = e;
        }
        if cursor < truncated.len() {
            spans.push(Span::styled(truncated[cursor..].to_string(), *base));
        }
    }
    let title_w = UnicodeWidthStr::width(truncated.as_str()) as u16;
    let used = icon_w.saturating_add(title_w);
    let pad = area_width.saturating_sub(used).saturating_sub(hint_w);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad as usize)));
    }
    if !hint.is_empty() {
        spans.push(Span::styled(hint.clone(), theme::palette_keyhint_style()));
    }
    spans
}

/// Read-only Help panel (`?`): Home (Active + TOC) or a zone page.
pub fn render_help_panel(app: &mut App, frame: &mut Frame, area: Rect) -> Option<Position> {
    let title = format!("{} Help", theme::GLYPH_HELP);
    let inner = render_modal_shell(&title, frame, area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let prompting = app.help_search.as_ref().is_some_and(|s| s.prompt);
    let chrome = crate::help::chrome_help_line();
    let chrome_idx = crate::help::home_active_len(app) + crate::help::HelpPage::ALL.len();
    let chrome_hits = crate::help::hits_on_line(app.help_search.as_ref(), None, chrome_idx);
    let chrome_line = crate::help::overlay_search_hits(chrome.line, &chrome_hits);

    let prompt_h = u16::from(prompting);
    let chrome_h = u16::from(!prompting);
    let reserved = prompt_h.saturating_add(chrome_h);
    let mut cursor = None;

    match app.help_view {
        crate::help::HelpView::Home { toc, toc_off } => {
            let active = crate::help::home_active_lines(app);
            let active_len = active.len();
            // Pin Active + chrome, but keep at least one TOC row when the
            // frame is short enough that a full Active block would hide it.
            let budget = inner.height.saturating_sub(reserved);
            let min_active = 1u16;
            let max_active = if budget >= (active_len as u16).saturating_add(1) {
                active_len as u16
            } else if budget > min_active {
                budget.saturating_sub(1).max(min_active)
            } else {
                budget.min(active_len as u16)
            };
            let toc_h = inner
                .height
                .saturating_sub(max_active.saturating_add(reserved));
            let view_h = toc_h as usize;
            let sel = toc as usize;
            let mut off = toc_off;
            if view_h > 0 {
                if sel < off {
                    off = sel;
                } else if sel >= off.saturating_add(view_h) {
                    off = sel + 1 - view_h;
                }
            }
            if let crate::help::HelpView::Home { toc_off, .. } = &mut app.help_view {
                *toc_off = off;
            }

            let chunks = Layout::vertical([
                Constraint::Length(max_active),
                Constraint::Min(0),
                Constraint::Length(prompt_h),
                Constraint::Length(chrome_h),
            ])
            .split(inner);

            let active_painted: Vec<Line<'static>> = active
                .into_iter()
                .enumerate()
                .map(|(i, row)| {
                    let hits = crate::help::hits_on_line(app.help_search.as_ref(), None, i);
                    crate::help::overlay_search_hits(row.line, &hits)
                })
                .collect();
            frame.render_widget(
                Paragraph::new(active_painted).wrap(ratatui::widgets::Wrap { trim: false }),
                chunks[0],
            );

            let toc_rows = crate::help::home_toc_lines(Some(toc));
            let visible: Vec<Line<'static>> = toc_rows
                .into_iter()
                .enumerate()
                .skip(off)
                .take(view_h)
                .map(|(i, row)| {
                    let hits =
                        crate::help::hits_on_line(app.help_search.as_ref(), None, active_len + i);
                    crate::help::overlay_search_hits(row.line, &hits)
                })
                .collect();
            frame.render_widget(Paragraph::new(visible), chunks[1]);

            if prompting {
                cursor = render_help_prompt(app, frame, chunks[2]);
            } else if chrome_h > 0 {
                frame.render_widget(Paragraph::new(chrome_line), chunks[3]);
            }
        }
        crate::help::HelpView::Page { id, scroll } => {
            let body = crate::help::page_doc_lines(app, id);
            let n = body.len();
            let chunks = Layout::vertical([
                Constraint::Min(1),
                Constraint::Length(prompt_h),
                Constraint::Length(chrome_h),
            ])
            .split(inner);
            let view_h = chunks[0].height as usize;
            app.help_body_view_h = view_h.max(1);
            let max_scroll = crate::help::page_max_scroll(n, app.help_body_view_h);
            let scroll = scroll.min(max_scroll);
            if let crate::help::HelpView::Page { scroll: stored, .. } = &mut app.help_view {
                *stored = scroll;
            }
            let painted: Vec<Line<'static>> = body
                .into_iter()
                .enumerate()
                .skip(scroll)
                .map(|(i, row)| {
                    let hits = crate::help::hits_on_line(app.help_search.as_ref(), Some(id), i);
                    crate::help::overlay_search_hits(row.line, &hits)
                })
                .collect();
            frame.render_widget(
                Paragraph::new(painted).wrap(ratatui::widgets::Wrap { trim: false }),
                chunks[0],
            );
            if prompting {
                cursor = render_help_prompt(app, frame, chunks[1]);
            } else if chrome_h > 0 {
                frame.render_widget(Paragraph::new(chrome_line), chunks[2]);
            }
        }
    }
    cursor
}

fn render_help_prompt(app: &App, frame: &mut Frame, area: Rect) -> Option<Position> {
    let Some(search) = app.help_search.as_ref() else {
        return None;
    };
    if area.height == 0 || area.width == 0 {
        return None;
    }
    let prefix = "/ ";
    let prefix_w = UnicodeWidthStr::width(prefix) as u16;
    let q_width = area.width.saturating_sub(prefix_w);
    let (q_spans, caret_col) =
        editable_text_spans(search.query.as_str(), search.query.cursor(), Some(q_width));
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        theme::context_help_style(),
    )];
    spans.extend(q_spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    Some(Position {
        x: area
            .x
            .saturating_add(prefix_w.saturating_add(caret_col.min(q_width.saturating_sub(1)))),
        y: area.y,
    })
}

/// Height for the Help modal given frame size and content.
pub fn help_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let max = frame.height.saturating_sub(4).max(8);
    let want = (content_rows as u16).saturating_add(2); // border
    want.min(max).max(8)
}

/// Vertically centered Help shell (unlike Input/Search which stay near the top).
pub fn help_modal_rect(frame: Rect, width: u16, content_rows: usize) -> Rect {
    centered_modal_rect(frame, width, help_modal_height(frame, content_rows))
}

/// Large rounded compare tray covering most of the frame (the main log).
pub fn compare_modal_rect(frame: Rect) -> Rect {
    let width = frame.width.saturating_sub(4).max(40);
    let height = frame.height.saturating_sub(4).max(8);
    centered_modal_rect(frame, width, height)
}

/// Bookmark compare list: log paint + Δt / stale prefixes. Always wraps.
pub fn render_compare_panel(app: &mut App, frame: &mut Frame, area: Rect) {
    let Some(_) = app.compare.as_ref() else {
        return;
    };
    let inner = render_modal_shell_glyph(theme::GLYPH_BOOKMARK, "Bookmark Compare", frame, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let sorted = app.bookmarks.sorted_indices();
    let deltas = app.bookmarks.delta_labels();
    let delta_col = deltas
        .iter()
        .map(|d| {
            d.as_deref()
                .unwrap_or("")
                .chars()
                .count()
                .max(theme::GLYPH_COMPARE_UNTIMED.chars().count())
        })
        .max()
        .unwrap_or(1);
    let prefix_w = 1 + 1 + delta_col + 1; // mark space delta space
    let entry_w = (inner.width as usize).saturating_sub(prefix_w).max(8);
    let patterns = app.highlight_groups.paint_patterns(app.active_highlight);
    let cursor = app.compare.as_ref().map(|p| p.cursor).unwrap_or(0);

    let mut items: Vec<ListItem> = Vec::with_capacity(sorted.len());
    for (display_i, &storage) in sorted.iter().enumerate() {
        let row = app.bookmarks.items[storage].row.clone();
        let row_id = row.row_id;
        let jumpable = app.visible_idx_for_row_id(row_id).is_some();
        let stale = !jumpable;
        let mark = if stale {
            theme::GLYPH_BOOKMARK_STALE_MARK
        } else {
            " "
        };
        let delta_text = deltas
            .get(display_i)
            .and_then(|d| d.as_deref())
            .unwrap_or("");
        let delta_pad = format!("{delta_text:>delta_col$}");
        let mut lines = render_compare_entry_lines(&row, &patterns, entry_w);
        if let Some(first) = lines.first_mut() {
            let mut prefix = vec![
                Span::styled(
                    format!("{mark} "),
                    if stale {
                        theme::bookmark_stale_style()
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(format!("{delta_pad} "), theme::compare_delta_style()),
            ];
            prefix.append(&mut first.spans);
            *first = Line::from(prefix);
        }
        let mut item = ListItem::new(lines);
        if display_i == cursor {
            item = item.style(theme::log_selection_style());
        }
        items.push(item);
    }

    let list = List::new(items);
    let mut state =
        ListState::default().with_offset(app.compare.as_ref().map(|p| p.list_offset).unwrap_or(0));
    if !sorted.is_empty() {
        state.select(Some(cursor.min(sorted.len() - 1)));
    }
    frame.render_stateful_widget(list, inner, &mut state);
    if let Some(panel) = app.compare.as_mut() {
        panel.list_offset = state.offset();
    }
}

const DASHBOARD_MAX_WIDTH: u16 = 72;

/// Full-frame startup Dashboard (unbound source), composed like a borderless
/// dashboard-nvim Hyper page rather than a popup.
pub fn render_dashboard(app: &App, frame: &mut Frame, area: Rect) {
    let Some(dash) = app.dashboard.as_ref() else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }

    clear_to_canvas(frame, area);
    let available_width = area.width.saturating_sub(4).max(1);
    let content_width = available_width.min(DASHBOARD_MAX_WIDTH).min(area.width);
    let density = crate::dashboard::DashboardDensity::for_size(content_width, area.height);
    let selected_recent =
        dash.cursor >= crate::dashboard::QUICK_ACTION_COUNT && dash.cursor < dash.len();
    let show_minimal_header = density == crate::dashboard::DashboardDensity::Minimal
        && area.height >= 5
        && (!selected_recent || area.height >= 6);
    let fixed_rows = density.fixed_rows(show_minimal_header).min(area.height);
    let recent_capacity = usize::from(area.height.saturating_sub(fixed_rows))
        .min(crate::dashboard::MAX_VISIBLE_RECENTS);
    let frame_height = fixed_rows
        .saturating_add(recent_capacity as u16)
        .min(area.height);
    let x = area.x + area.width.saturating_sub(content_width) / 2;
    let y = if density == crate::dashboard::DashboardDensity::Full {
        area.y + area.height.saturating_sub(frame_height) / 2
    } else {
        area.y + u16::from(area.height > frame_height)
    };
    let content = Rect::new(x, y, content_width, frame_height);

    let items = dash.items();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(frame_height as usize);

    match density {
        crate::dashboard::DashboardDensity::Full => {
            lines.extend(
                theme::DASHBOARD_LOGO
                    .into_iter()
                    .enumerate()
                    .map(|(i, row)| {
                        Line::from(Span::styled(row, theme::dashboard_logo_line_style(i)))
                            .alignment(Alignment::Center)
                    }),
            );
            lines.push(
                Line::from(Span::styled(
                    "App / Android Log Navigator",
                    theme::dashboard_muted_style(),
                ))
                .alignment(Alignment::Center),
            );
            lines.push(Line::from(""));
        }
        crate::dashboard::DashboardDensity::Compact => {
            lines.push(
                Line::from(Span::styled("alnav", theme::dashboard_header_style()))
                    .alignment(Alignment::Center),
            );
        }
        crate::dashboard::DashboardDensity::Minimal if show_minimal_header => {
            lines.push(
                Line::from(Span::styled("alnav", theme::dashboard_header_style()))
                    .alignment(Alignment::Center),
            );
        }
        crate::dashboard::DashboardDensity::Minimal => {}
    }

    if density != crate::dashboard::DashboardDensity::Minimal {
        lines.push(dashboard_section_line(
            theme::GLYPH_TITLE_DASHBOARD,
            "Quick Actions",
        ));
    }
    for (idx, item) in items
        .iter()
        .take(crate::dashboard::QUICK_ACTION_COUNT)
        .enumerate()
    {
        lines.push(dashboard_item_line(
            item,
            idx == dash.cursor,
            content_width,
            density.show_descriptions(),
        ));
    }

    let range = dash.visible_recent_range(recent_capacity);
    if density != crate::dashboard::DashboardDensity::Minimal {
        let recent_title = if dash.recent.paths.len() > recent_capacity && !range.is_empty() {
            format!(
                "Recent Files  {}-{} / {}",
                range.start + 1,
                range.end,
                dash.recent.paths.len()
            )
        } else {
            "Recent Files".to_string()
        };
        lines.push(dashboard_section_line(
            theme::GLYPH_SOURCE_RECENT,
            &recent_title,
        ));
    }

    if dash.recent.paths.is_empty() {
        if recent_capacity > 0 {
            lines.push(Line::from(Span::styled(
                "No recent files yet",
                theme::dashboard_muted_style(),
            )));
            lines.extend(
                std::iter::repeat_with(|| Line::from("")).take(recent_capacity.saturating_sub(1)),
            );
        }
    } else {
        for recent_idx in range.clone() {
            let item_idx = crate::dashboard::QUICK_ACTION_COUNT + recent_idx;
            if let Some(item) = items.get(item_idx) {
                lines.push(dashboard_item_line(
                    item,
                    item_idx == dash.cursor,
                    content_width,
                    false,
                ));
            }
        }
        lines.extend(
            std::iter::repeat_with(|| Line::from(""))
                .take(recent_capacity.saturating_sub(range.len())),
        );
    }

    lines.push(
        Line::from(
            app.status_msg
                .as_deref()
                .map(|msg| Span::styled(msg.to_string(), theme::dashboard_flash_style()))
                .unwrap_or_else(|| Span::raw("")),
        )
        .alignment(Alignment::Center),
    );
    if density != crate::dashboard::DashboardDensity::Minimal {
        lines.push(
            Line::from(Span::styled(
                format!(
                    "j/k move  ·  Enter open  ·  q quit  ·  alnav v{}",
                    env!("CARGO_PKG_VERSION")
                ),
                theme::dashboard_muted_style(),
            ))
            .alignment(Alignment::Center),
        );
    }

    lines.truncate(content.height as usize);
    frame.render_widget(Paragraph::new(lines), content);
}

fn dashboard_section_line(glyph: &'static str, title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("{glyph}  {title}"),
        theme::dashboard_section_style(),
    ))
}

fn dashboard_item_line(
    item: &crate::dashboard::DashboardItem,
    selected: bool,
    width: u16,
    show_description: bool,
) -> Line<'static> {
    let row_style = theme::dashboard_item_style(selected);
    let marker = if selected {
        theme::candidate_prefix()
    } else {
        " ".repeat(UnicodeWidthStr::width(theme::candidate_prefix().as_str()).max(1))
    };
    let prefix = format!("{marker}{}  ", item.glyph());
    let hotkey = item
        .hotkey()
        .map(|key| format!("[{key}]"))
        .unwrap_or_default();
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let hotkey_width = UnicodeWidthStr::width(hotkey.as_str());
    let hotkey_gap = usize::from(!hotkey.is_empty());
    let label_budget = usize::from(width)
        .saturating_sub(prefix_width)
        .saturating_sub(hotkey_width)
        .saturating_sub(hotkey_gap);

    let mut spans = vec![Span::styled(prefix, row_style)];
    let label_width = match item {
        crate::dashboard::DashboardItem::Recent { path, .. } => {
            let (basename, parent) = dashboard_recent_parts(path, label_budget);
            let basename_width = UnicodeWidthStr::width(basename.as_str());
            spans.push(Span::styled(basename, row_style));
            if parent.is_empty() {
                basename_width
            } else {
                let parent_text = format!("  {parent}");
                let width = basename_width + UnicodeWidthStr::width(parent_text.as_str());
                spans.push(Span::styled(
                    parent_text,
                    theme::dashboard_description_style(selected),
                ));
                width
            }
        }
        _ => {
            let title = fit_display_end(&item.label(), label_budget);
            let mut used = UnicodeWidthStr::width(title.as_str());
            spans.push(Span::styled(title, row_style));
            if show_description {
                if let Some(description) = item.description() {
                    let remaining = label_budget.saturating_sub(used);
                    let separator = " — ";
                    let separator_width = UnicodeWidthStr::width(separator);
                    if remaining > separator_width {
                        let description = fit_display_end(description, remaining - separator_width);
                        let text = format!("{separator}{description}");
                        used += UnicodeWidthStr::width(text.as_str());
                        spans.push(Span::styled(
                            text,
                            theme::dashboard_description_style(selected),
                        ));
                    }
                }
            }
            used
        }
    };

    let used = prefix_width + label_width + hotkey_width + hotkey_gap;
    spans.push(Span::styled(
        " ".repeat(usize::from(width).saturating_sub(used)),
        row_style,
    ));
    if !hotkey.is_empty() {
        spans.push(Span::styled(" ", row_style));
        spans.push(Span::styled(
            hotkey,
            theme::dashboard_hotkey_style(selected),
        ));
    }
    Line::from(spans)
}

fn dashboard_recent_parts(path: &str, max_width: usize) -> (String, String) {
    let path_ref = std::path::Path::new(path);
    let basename = path_ref
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    let basename = fit_display_end(basename, max_width);
    let basename_width = UnicodeWidthStr::width(basename.as_str());
    if basename_width >= max_width {
        return (basename, String::new());
    }

    let parent = dashboard_parent_label(path_ref);
    let parent_budget = max_width.saturating_sub(basename_width + 2);
    if parent_budget < 3 {
        return (basename, String::new());
    }
    (basename, fit_display_middle(&parent, parent_budget))
}

fn dashboard_parent_label(path: &std::path::Path) -> String {
    let Some(parent) = path.parent() else {
        return String::new();
    };
    let mut label = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .and_then(|home| {
            parent.strip_prefix(&home).ok().map(|relative| {
                if relative.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    format!("~/{}", relative.display())
                }
            })
        })
        .unwrap_or_else(|| parent.display().to_string());
    if !label.ends_with(std::path::MAIN_SEPARATOR) {
        label.push(std::path::MAIN_SEPARATOR);
    }
    label
}

fn fit_display_end(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let mut out = take_display_prefix(text, max_width - 1);
    out.push('…');
    out
}

fn fit_display_middle(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let left_width = (max_width - 1) / 2;
    let right_width = max_width - 1 - left_width;
    format!(
        "{}…{}",
        take_display_prefix(text, left_width),
        take_display_suffix(text, right_width)
    )
}

fn take_display_prefix(text: &str, max_width: usize) -> String {
    let mut used = 0;
    text.graphemes(true)
        .take_while(|grapheme| {
            let width = UnicodeWidthStr::width(*grapheme);
            let fits = used + width <= max_width;
            if fits {
                used += width;
            }
            fits
        })
        .collect::<String>()
}

fn take_display_suffix(text: &str, max_width: usize) -> String {
    let mut used = 0;
    let mut graphemes: Vec<&str> = text
        .graphemes(true)
        .rev()
        .take_while(|grapheme| {
            let width = UnicodeWidthStr::width(*grapheme);
            let fits = used + width <= max_width;
            if fits {
                used += width;
            }
            fits
        })
        .collect();
    graphemes.reverse();
    graphemes.concat()
}

/// Open-file left pane: at least half width so basename-first labels stay readable.
const OPEN_FILE_LEFT_RATIO_FLOOR: f32 = 0.55;

/// Open-file source panel (`C-f`): left candidates + draft, right full path.
pub fn render_open_file_panel(
    panel: &crate::source_panel::OpenFilePanel,
    left_ratio: f32,
    frame: &mut Frame,
    frame_area: Rect,
) -> Option<Position> {
    let labels: Vec<String> = panel
        .choices
        .iter()
        .map(|c| {
            let glyph = match c {
                crate::source_panel::OpenFileChoice::Recent(_) => theme::GLYPH_SOURCE_RECENT,
                crate::source_panel::OpenFileChoice::Corpus { .. } => theme::GLYPH_SOURCE_FILE,
            };
            format!("{glyph} {}", c.display_label())
        })
        .collect();
    let styles = vec![Style::default(); labels.len()];
    let checked = vec![false; labels.len()];
    let actions = vec![ActionKind::None; labels.len()];
    let mut right_lines: Vec<Line<'static>> = Vec::new();
    if let Some(status) = &panel.corpus_status {
        right_lines.push(Line::from(Span::styled(status.clone(), theme::muted())));
        right_lines.push(Line::from(""));
    }
    match panel.selected_full_path() {
        Some(path) => {
            right_lines.push(Line::from(Span::styled(
                "full path".to_string(),
                theme::muted().add_modifier(Modifier::DIM),
            )));
            right_lines.push(Line::from(Span::raw(path)));
            if let Some(label) = panel.selected_corpus_label() {
                right_lines.push(Line::from(""));
                right_lines.push(Line::from(Span::styled(
                    "corpus".to_string(),
                    theme::muted().add_modifier(Modifier::DIM),
                )));
                right_lines.push(Line::from(Span::styled(label, theme::muted())));
            }
        }
        None => {
            right_lines.push(Line::from(Span::styled(
                "no selection".to_string(),
                theme::preview_placeholder_style(),
            )));
        }
    }
    let title = format!("{} Open file", theme::GLYPH_SOURCE_OPEN_FILE);
    let mode = crate::picker::PickerMode::New;
    let open_left = left_ratio.max(OPEN_FILE_LEFT_RATIO_FLOOR);
    render_picker(
        &title,
        &mode,
        panel.draft.as_str(),
        panel.draft.cursor(),
        panel.draft.as_str(),
        &[],
        false,
        None,
        &labels,
        &styles,
        &checked,
        &actions,
        panel.selected,
        "scanning log_dirs… or configure log_dirs · Ctrl-r refresh",
        PickerRightPane::PlainLines(&right_lines),
        open_left,
        true,
        Some(theme::GLYPH_SOURCE_OPEN_FILE),
        frame,
        frame_area,
    )
}

/// Centered HDC / ADB chooser (`C-g`).
pub fn render_stream_source_panel(
    panel: &crate::source_panel::StreamSourcePanel,
    frame: &mut Frame,
    frame_area: Rect,
) {
    let width = modal_width(frame_area.width).min(40);
    let height = 8;
    let area = centered_modal_rect(frame_area, width, height);
    let title = format!("{} Stream source", theme::GLYPH_SOURCE_HDC);
    let inner = render_modal_shell(&title, frame, area);
    let rows = [
        (0usize, theme::GLYPH_SOURCE_HDC, "HDC  hilog", "h"),
        (1usize, theme::GLYPH_SOURCE_ADB, "ADB  logcat", "a"),
    ];
    let mut lines = vec![Line::from(Span::styled(
        "j/k move  ·  Enter / h / a select  ·  Esc cancel",
        theme::muted(),
    ))];
    lines.push(Line::from(""));
    for (idx, glyph, label, hot) in rows {
        let selected = panel.selected == idx;
        let style = if selected {
            theme::candidate_selected_style()
        } else {
            theme::candidate_unselected_style()
        };
        let marker = if selected {
            theme::candidate_prefix()
        } else {
            " ".repeat(theme::candidate_prefix().chars().count().max(1))
        };
        lines.push(Line::from(vec![
            Span::styled(marker, style),
            Span::styled(format!("[{hot}] "), theme::muted()),
            Span::styled(format!("{glyph} {label}"), style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight_model::HighlightGroup;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn cell_text(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn render_help_text(app: &mut App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_help_panel(app, frame, frame.area());
            })
            .unwrap();
        cell_text(terminal.backend().buffer())
    }

    #[test]
    fn help_home_shows_numbered_filter_toc() {
        let mut app = App::new(100);
        app.open_help();
        let text = render_help_text(&mut app, 80, 24);
        assert!(text.contains("1"), "{text}");
        assert!(text.contains("Filter"), "{text}");
        assert!(text.contains("Active"), "{text}");
    }

    #[test]
    fn help_short_frame_keeps_active_chrome_and_toc() {
        let mut app = App::new(100);
        app.open_help();
        let text = render_help_text(&mut app, 80, 8);
        assert!(text.contains("Active"), "{text}");
        assert!(
            text.contains("close") || text.contains("Esc"),
            "chrome should stay pinned: {text}"
        );
        assert!(
            text.contains("Log") || text.contains("Filter"),
            "TOC should keep a visible row: {text}"
        );
    }

    #[test]
    fn help_exclude_page_shows_blurb() {
        let mut app = App::new(100);
        app.open_help();
        app.help_open_page(crate::help::HelpPage::Exclude);
        let text = render_help_text(&mut app, 80, 24);
        assert!(
            text.contains("AND NOT") || text.contains("Exclude"),
            "{text}"
        );
    }

    #[test]
    fn help_modal_rect_is_vertically_centered() {
        let frame = Rect::new(0, 0, 80, 40);
        let rect = help_modal_rect(frame, 56, 10);
        let top = top_modal_rect(frame, 56, rect.height);
        assert!(rect.y > top.y, "help should sit below top-aligned modals");
        assert_eq!(rect.y, (frame.height - rect.height) / 2);
        assert_eq!(rect.x, (frame.width - rect.width) / 2);
    }

    #[test]
    fn help_page_render_clamps_scroll_to_viewport() {
        let mut app = App::new(100);
        app.open_help();
        app.help_open_page(crate::help::HelpPage::Log);
        app.help_view = crate::help::HelpView::Page {
            id: crate::help::HelpPage::Log,
            scroll: 10_000,
        };
        let _ = render_help_text(&mut app, 80, 24);
        let n = crate::help::page_doc_lines(&app, crate::help::HelpPage::Log).len();
        let crate::help::HelpView::Page { scroll, .. } = app.help_view else {
            panic!("expected page");
        };
        assert!(
            scroll <= crate::help::page_max_scroll(n, app.help_body_view_h),
            "scroll={scroll} n={n} view={}",
            app.help_body_view_h
        );
        assert!(app.help_body_view_h > 1);
    }

    #[test]
    fn help_search_hit_uses_theme_style() {
        let mut app = App::new(100);
        app.open_help();
        app.help_begin_search();
        if let Some(s) = app.help_search.as_mut() {
            s.query.set_text("chip");
        }
        app.help_rebuild_search();
        if let Some(s) = app.help_search.as_mut() {
            s.prompt = false;
        }
        app.help_jump_current_hit();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_help_panel(&mut app, frame, frame.area());
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let want = theme::help_search_hit_style()
            .bg
            .or(theme::help_search_current_style().bg);
        let mut found = false;
        for y in buf.area.y..buf.area.y + buf.area.height {
            for x in buf.area.x..buf.area.x + buf.area.width {
                let cell = &buf[(x, y)];
                if cell.symbol().eq_ignore_ascii_case("c") && Some(cell.bg) == want {
                    found = true;
                }
            }
        }
        assert!(found, "expected a help search hit cell with highlight bg");
    }

    fn row0_bg_at_needle(buf: &ratatui::buffer::Buffer, needle: &str) -> Option<Color> {
        let mut text = String::new();
        let mut starts = Vec::new();
        for x in 0..buf.area.width {
            starts.push(text.len());
            text.push_str(buf[(x, 0)].symbol());
        }
        let i = text.find(needle)?;
        let x = starts.iter().rposition(|&s| s <= i)? as u16;
        Some(buf[(x, 0)].bg)
    }

    fn dashboard_app(paths: Vec<String>) -> App {
        let mut app = App::new(100);
        let recent = crate::recent::RecentFiles { paths };
        app.recent = recent.clone();
        app.dashboard = Some(crate::dashboard::DashboardState::new(recent));
        app
    }

    fn render_dashboard_text(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_dashboard(app, frame, frame.area()))
            .unwrap();
        cell_text(terminal.backend().buffer())
    }

    #[test]
    fn dashboard_full_logo_uses_per_line_theme_colors() {
        let p = crate::theme_builtins::palette_by_name("dracula").unwrap();
        crate::theme::install(crate::theme::map_to_tokens_for(&p, "dracula"));
        let app = dashboard_app(Vec::new());
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_dashboard(&app, frame, frame.area()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let fg_on_row_containing = |needle: &str| -> Option<ratatui::style::Color> {
            for y in buf.area.y..buf.area.y + buf.area.height {
                let mut line = String::new();
                let mut sample = None;
                for x in buf.area.x..buf.area.x + buf.area.width {
                    let cell = &buf[(x, y)];
                    line.push_str(cell.symbol());
                    if sample.is_none() && cell.symbol() != " " {
                        sample = Some(cell.fg);
                    }
                }
                if line.contains(needle) {
                    return sample;
                }
            }
            None
        };
        let top = fg_on_row_containing("█████╗");
        let bottom = fg_on_row_containing("╚══════╝");
        crate::theme::install(crate::theme::UiTokens::builtin());
        assert_eq!(top, Some(p.cyan));
        assert_eq!(bottom, Some(p.blue));
        assert_ne!(top, bottom);
    }

    #[test]
    fn dashboard_renders_hyper_sections_and_footer() {
        let app = dashboard_app(vec!["/tmp/app.log".into()]);
        let text = render_dashboard_text(&app, 100, 30);

        assert!(text.contains("█████╗"));
        assert!(text.contains("╚══════╝"));
        assert!(text.contains("App / Android Log Navigator"));
        assert!(text.contains("Quick Actions"));
        assert!(text.contains("HDC — HarmonyOS hilog"));
        assert!(text.contains("ADB — Android logcat"));
        assert!(text.contains("Open file — Recent + configured log_dirs (fuzzy)"));
        assert!(text.contains("Recent Files"));
        assert!(text.contains("j/k move"));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn dashboard_standard_terminal_keeps_full_header_and_footer() {
        let app = dashboard_app(Vec::new());
        let text = render_dashboard_text(&app, 80, 24);

        assert!(text.contains("█████╗"));
        assert!(text.contains("╚══════╝"));
        assert!(text.contains("App / Android Log Navigator"));
        assert!(text.contains("Quick Actions"));
        assert!(text.contains("Recent Files"));
        assert!(text.contains("j/k move"));
    }

    #[test]
    fn dashboard_empty_recent_keeps_placeholder() {
        let app = dashboard_app(Vec::new());
        let text = render_dashboard_text(&app, 80, 24);

        assert!(text.contains("Recent Files"));
        assert!(text.contains("No recent files yet"));
    }

    #[test]
    fn dashboard_short_frame_keeps_selected_recent_visible() {
        let paths = (1..=20).map(|i| format!("/tmp/file-{i:02}.log")).collect();
        let mut app = dashboard_app(paths);
        app.dashboard.as_mut().unwrap().cursor = 22;

        let text = render_dashboard_text(&app, 48, 12);
        assert!(text.contains("file-20.log"));
    }

    #[test]
    fn dashboard_formats_home_path_and_ellipsizes_parent() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/test".into());
        let path =
            format!("{home}/Work/a-very-long-project-name/another-long-directory/device/app.log");
        let app = dashboard_app(vec![path]);

        let text = render_dashboard_text(&app, 48, 24);
        assert!(text.contains("app.log"));
        assert!(text.contains("~/"));
        assert!(text.contains('…'));
    }

    #[test]
    fn dashboard_renders_flash_in_reserved_row() {
        let mut app = dashboard_app(Vec::new());
        app.set_flash("HDC CONNECT FAILED");

        let text = render_dashboard_text(&app, 80, 24);
        assert!(text.contains("HDC CONNECT FAILED"));
    }

    #[test]
    fn dashboard_recent_window_reports_range_and_caps_at_nine() {
        let paths = (1..=20).map(|i| format!("/tmp/file-{i:02}.log")).collect();
        let app = dashboard_app(paths);
        let text = render_dashboard_text(&app, 100, 30);

        assert!(text.contains("Recent Files  1-9 / 20"));
        assert!(text.contains("file-09.log"));
        assert!(!text.contains("file-10.log"));
    }

    #[test]
    fn dashboard_hotkeys_are_right_aligned_in_content_column() {
        let app = dashboard_app(Vec::new());
        let text = render_dashboard_text(&app, 100, 30);
        let hdc_line = text.lines().find(|line| line.contains("HDC —")).unwrap();
        let hotkey_byte = hdc_line.find("[h]").unwrap();
        let hotkey_column = UnicodeWidthStr::width(&hdc_line[..hotkey_byte]);

        assert_eq!(hotkey_column, 83); // centered x=14 + (72 - "[h]".width)
    }

    #[test]
    fn dashboard_minimal_keeps_wordmark_actions_and_selected_recent() {
        let mut app = dashboard_app(vec!["/tmp/selected.log".into()]);
        let text = render_dashboard_text(&app, 30, 8);
        assert!(text.contains("alnav"));
        assert!(text.contains("HDC"));
        assert!(text.contains("ADB"));
        assert!(text.contains("Open file"));

        app.dashboard.as_mut().unwrap().cursor = 3;
        let text = render_dashboard_text(&app, 30, 5);
        assert!(text.contains("HDC"));
        assert!(text.contains("ADB"));
        assert!(text.contains("Open file"));
        assert!(text.contains("selected.log"));
        assert!(!text.contains("alnav"));
    }

    #[test]
    fn dashboard_flash_row_does_not_shift_footer() {
        let app = dashboard_app(Vec::new());
        let without_flash = render_dashboard_text(&app, 80, 24);
        let footer_before = without_flash
            .lines()
            .position(|line| line.contains("j/k move"))
            .unwrap();

        let mut app = app;
        app.set_flash("ADB CONNECT FAILED");
        let with_flash = render_dashboard_text(&app, 80, 24);
        let footer_after = with_flash
            .lines()
            .position(|line| line.contains("j/k move"))
            .unwrap();

        assert_eq!(footer_before, footer_after);
        assert!(with_flash.contains("ADB CONNECT FAILED"));
    }

    #[test]
    fn dashboard_unicode_truncation_preserves_emoji_graphemes() {
        let end = fit_display_end("👩‍💻.log", 3);
        assert_eq!(end, "👩‍💻…");
        assert_eq!(UnicodeWidthStr::width(end.as_str()), 3);

        let middle = fit_display_middle("~/项目/👩‍💻/nearest/", 12);
        assert!(UnicodeWidthStr::width(middle.as_str()) <= 12);
        assert!(!middle.contains("👩‍") || middle.contains("👩‍💻"));
    }

    #[test]
    fn candidate_viewport_range_bounds_paint_window() {
        assert_eq!(candidate_viewport_range(0, 0, 10), (0, 0));
        assert_eq!(candidate_viewport_range(5, 0, 10), (0, 5));
        assert_eq!(candidate_viewport_range(100, 0, 8), (0, 8));
        let (off, end) = candidate_viewport_range(100, 50, 8);
        assert_eq!(end - off, 8);
        assert!(off <= 50 && 50 < end);
        let (off, end) = candidate_viewport_range(100, 99, 8);
        assert_eq!(end, 100);
        assert_eq!(end - off, 8);
    }

    #[test]
    fn candidate_list_viewport_paint_stays_fast_with_many_labels() {
        use std::time::Instant;
        let labels: Vec<String> = (0..crate::fuzzy::CANDIDATE_RESULT_CAP)
            .map(|i| format!("CandidateLabel_{i:03}"))
            .collect();
        let styles = vec![Style::default(); labels.len()];
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let t = Instant::now();
        terminal
            .draw(|f| {
                render_candidate_list(
                    "list",
                    &labels,
                    &styles,
                    &[],
                    &[],
                    200,
                    "empty",
                    "Cand",
                    f,
                    f.area(),
                    false,
                );
            })
            .unwrap();
        // ViewportPaint: even 256 labels must not approach the old O(n) ~90ms path.
        assert!(
            t.elapsed().as_millis() < 50,
            "viewport paint took {:?} (expected << 50ms)",
            t.elapsed()
        );
    }

    fn fg_of_needle(buf: &ratatui::buffer::Buffer, needle: &str) -> Option<Color> {
        for y in 0..buf.area.height {
            let mut text = String::new();
            let mut starts = Vec::new();
            for x in 0..buf.area.width {
                starts.push(text.len());
                text.push_str(buf[(x, y)].symbol());
            }
            if let Some(i) = text.find(needle) {
                let x = starts.iter().rposition(|&s| s <= i)? as u16;
                return Some(buf[(x, y)].fg);
            }
        }
        None
    }

    fn render_lines(app: &mut App, lines: &[&str]) -> ratatui::buffer::Buffer {
        let (tx, rx) = std::sync::mpsc::channel();
        for line in lines {
            tx.send(crate::model::EntryRow::from_line(line).unwrap())
                .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.focus = Focus::Input;
        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(app, frame, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn test_render_log_list_shows_tag_and_msg() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello world")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("MyTag"));
        assert!(content.contains("hello world"));
    }

    #[test]
    fn error_and_crash_rows_paint_tag_and_msg_in_theme_red() {
        crate::theme::install(crate::theme::UiTokens::builtin());
        let mut app = App::new(100);
        let buf = render_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I InfoTag : UNIQUEINFOMSG",
                "04-02 10:00:01.000  1  1 E ErrTag  : UNIQUEERRMSG",
                "04-02 10:00:02.000  1  1 F FatTag  : UNIQUEFATALMSG",
                "04-02 10:00:03.000  1  1 I AndroidRuntime: FATAL EXCEPTION: main UNIQUECRASH",
            ],
        );
        assert_ne!(
            fg_of_needle(&buf, "UNIQUEINFOMSG"),
            Some(Color::Red),
            "info msg must not use error red"
        );
        assert_eq!(fg_of_needle(&buf, "UNIQUEERRMSG"), Some(Color::Red));
        assert_eq!(fg_of_needle(&buf, "ErrTag"), Some(Color::Red));
        assert_eq!(fg_of_needle(&buf, "UNIQUEFATALMSG"), Some(Color::Red));
        assert_eq!(fg_of_needle(&buf, "UNIQUECRASH"), Some(Color::Red));
    }

    #[test]
    fn test_render_log_list_highlights_selected_row_with_soft_gray_when_focused() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first")
                .unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 1; // select the second row

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        // Row 0 is the block's top border; content starts at y=1 (one row
        // per entry, no wrapping for these short messages) and x=1 (past
        // the left border column).
        let buf = terminal.backend().buffer();
        let selected_style = buf[(1, 2)].style();
        let unselected_style = buf[(1, 1)].style();
        assert_eq!(
            selected_style.bg,
            Some(Color::DarkGray),
            "focused selection must use the soft gray background"
        );
        assert_ne!(
            unselected_style.bg,
            Some(Color::DarkGray),
            "unselected rows must not get the selection background"
        );
        assert!(
            !selected_style.add_modifier.contains(Modifier::REVERSED),
            "must not use the old reverse-video style"
        );
    }

    #[test]
    fn test_render_log_list_no_highlight_when_log_list_unfocused() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first")
                .unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 1;
        app.focus = Focus::Input; // LogList no longer has keyboard focus

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        let buf = terminal.backend().buffer();
        let selected_style = buf[(1, 2)].style();
        let unselected_style = buf[(1, 1)].style();
        assert_eq!(
            selected_style, unselected_style,
            "with LogList unfocused, the previously-selected row must look identical to any other row"
        );
    }

    #[test]
    fn test_selection_preserves_keyword_highlight_bg() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error here")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("error").unwrap());
        app.focus = Focus::LogList;
        app.cursor = 0;

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        let expected_hl = theme::highlight_style(0).bg;
        let buf = terminal.backend().buffer();
        // Scan the content row for a cell whose bg is the highlight color.
        let mut found = false;
        for x in 0..buf.area.width {
            if buf[(x, 1)].style().bg == expected_hl {
                found = true;
                break;
            }
        }
        assert!(found, "keyword highlight bg must survive selection overlay");
    }
    #[test]
    fn test_render_log_list_bookmark_row_bg_priority() {
        // AC1: bookmarked rows get a faint-yellow bg; an active visual selection
        // overrides it; the cursor-selection gray only applies when neither
        // visual nor bookmark bg is present. Priority: visual > bookmark-bg > cursor.
        // The whole buffer is scanned (bookmark strip shifts list content, so
        // fixed row coords are unreliable).
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA   : first").unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I TagB   : second")
                .unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I TagC   : third").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;

        // Bookmark rows 0 and 1; row 2 stays plain.
        app.cursor = 0;
        app.bookmark_add_current();
        app.cursor = 1;
        let bm_bg = theme::bookmark_row_style().bg;
        let vis_bg = theme::log_visual_style().bg;
        let sel_bg = theme::log_selection_style().bg;

        let scan_bg_in_rows =
            |terminal: &Terminal<TestBackend>, target: Option<Color>, y0: u16, y1: u16| -> bool {
                let buf = terminal.backend().buffer();
                for y in y0..y1 {
                    for x in 0..buf.area.width {
                        if buf[(x, y)].style().bg == target {
                            return true;
                        }
                    }
                }
                false
            };

        // Case 1: cursor on row 2 (focused, no visual selection). Rows 0 and 1
        // are bookmarked but not the cursor row → they get the bookmark bg;
        // the cursor row gets the selection gray.
        app.cursor = 2;
        app.focus = Focus::LogList;
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        assert!(
            scan_bg_in_rows(&terminal, bm_bg, 0, terminal.backend().buffer().area.height),
            "bookmarked non-cursor row must get the bookmark bg"
        );
        assert!(
            scan_bg_in_rows(
                &terminal,
                sel_bg,
                0,
                terminal.backend().buffer().area.height
            ),
            "focused cursor row must get the selection bg"
        );

        // Case 2: enter visual-line on row 0, extend cursor to row 1. Both
        // bookmarked rows are inside the visual range → visual overrides bookmark.
        // The bookmark strip occupies the top rows (1 border + 2 strip rows),
        // so list content starts at y=3; rows 0,1 land at y=3,4 and must carry
        // the visual bg, NOT the bookmark bg.
        app.enter_visual_line(); // anchor at row 0
        app.cursor = 1; // range [0,1]
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        assert!(
            scan_bg_in_rows(
                &terminal,
                vis_bg,
                3,
                terminal.backend().buffer().area.height
            ),
            "visual selection must override bookmark bg on list rows"
        );
        assert!(
            !scan_bg_in_rows(&terminal, bm_bg, 3, terminal.backend().buffer().area.height),
            "bookmark bg must yield to visual selection on list rows"
        );
    }

    #[test]
    fn test_render_log_list_persists_scroll_offset_when_cursor_moves_within_viewport() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..30 {
            tx.send(
                crate::model::EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 I Tag     : line{i}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.following = false;

        // Backend height 7 minus 2 border rows leaves a 5-row inner viewport.
        let backend = TestBackend::new(80, 7);
        let mut terminal = Terminal::new(backend).unwrap();

        app.cursor = 20;
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        let offset_at_edge = app.list_offset;
        assert!(
            offset_at_edge > 0,
            "cursor near the bottom of a 30-row list in a 5-row viewport must have scrolled"
        );

        app.cursor -= 2; // moves, but stays inside the already-scrolled viewport
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        assert_eq!(
            app.list_offset, offset_at_edge,
            "moving within the visible window must not re-scroll the viewport"
        );
    }

    #[test]
    fn test_wrap_ranges_breaks_on_whitespace() {
        let ranges = wrap_ranges("hello world foo", 11);
        let chunks: Vec<&str> = ranges
            .iter()
            .map(|&(s, e)| &"hello world foo"[s..e])
            .collect();
        assert_eq!(chunks, vec!["hello world", "foo"]);
    }

    #[test]
    fn test_wrap_ranges_hard_cuts_overlong_word() {
        let text = "supercalifragilistic";
        let ranges = wrap_ranges(text, 6);
        assert!(
            ranges.len() > 1,
            "an overlong word must be split into multiple pieces"
        );
        for &(s, e) in &ranges {
            assert!(e - s <= 6);
        }
        let rejoined: String = ranges.iter().map(|&(s, e)| &text[s..e]).collect();
        assert_eq!(rejoined, text);
    }

    #[test]
    fn test_wrap_ranges_short_text_is_single_range() {
        let ranges = wrap_ranges("short", 80);
        assert_eq!(ranges, vec![(0, 5)]);
    }

    #[test]
    fn test_render_entry_line_single_truncates_msg_with_ellipsis() {
        let long = "word ".repeat(40);
        let line = format!("04-02 10:00:00.000  1  1 I Tag     : {long}");
        let row = EntryRow::from_line(&line).unwrap();
        let rendered = render_entry_line_single(&row, &[], 60);
        let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains('…'),
            "single-line preview must ellipsize: {text}"
        );
        assert!(
            !text.contains('\n'),
            "single-line preview must stay one physical line"
        );
        assert!(
            text.starts_with("04-02 10:00:00.000"),
            "must start at timestamp"
        );
    }

    #[test]
    fn test_render_entry_line_single_highlights_pattern() {
        let row =
            EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred").unwrap();
        let patterns = [("error", 0usize, true)];
        let rendered = render_entry_line_single(&row, &patterns, 200);
        let matched = rendered
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "error")
            .expect("highlight span");
        assert_eq!(matched.style, theme::highlight_style_active(0));
    }

    #[test]
    fn test_render_entry_lines_wraps_long_message_into_multiple_lines() {
        let row = EntryRow::from_line(
            "04-02 10:00:00.000  1  1 I Tag     : this message is long enough that it must wrap across more than one physical line when the column width is narrow",
        )
        .unwrap();
        let lines = render_entry_lines(&row, &[], 40, 1, 1);
        assert!(
            lines.len() > 1,
            "a long message should wrap into multiple lines, got {}",
            lines.len()
        );
    }

    #[test]
    fn test_render_entry_lines_highlights_only_matched_keyword() {
        let row =
            EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred here")
                .unwrap();
        let patterns = [("error", 0usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        assert_eq!(lines.len(), 1);
        let matched: Vec<&Span> = lines[0]
            .spans
            .iter()
            .filter(|s| s.content.as_ref() == "error")
            .collect();
        assert_eq!(
            matched.len(),
            1,
            "exactly the matched keyword should be its own span"
        );
        let other_span_styles: Vec<Style> = lines[0]
            .spans
            .iter()
            .filter(|s| s.content.as_ref() != "error")
            .map(|s| s.style)
            .collect();
        assert!(
            other_span_styles.iter().all(|s| *s != matched[0].style),
            "non-matched spans must not share the highlight style"
        );
    }

    #[test]
    fn test_render_entry_lines_highlights_tag_matches() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello world").unwrap();
        let patterns = [("tag", 0usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        let matched = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "Tag")
            .expect("tag substring should be its own highlighted span");
        assert_eq!(matched.style, theme::highlight_style(0));
        let prefix = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "My")
            .expect("unmatched tag prefix keeps accent style");
        assert_eq!(
            prefix.style,
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_render_entry_lines_multicolor_patterns() {
        let row =
            EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : foo and bar here").unwrap();
        let patterns = [("foo", 0usize, true), ("bar", 1usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        let foo = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "foo")
            .unwrap();
        let bar = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "bar")
            .unwrap();
        assert_eq!(foo.style, theme::highlight_style_active(0));
        assert_eq!(bar.style, theme::highlight_style(1));
        assert!(foo.style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(!bar.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_ne!(foo.style.bg, bar.style.bg);
    }

    #[test]
    fn test_render_entry_lines_pads_short_tag_to_fixed_column() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Ab   : msg").unwrap();
        let lines = render_entry_lines(&row, &[], 200, 1, 1);
        let tag_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref().starts_with("Ab"))
            .expect("tag span");
        assert_eq!(
            tag_span.content.as_ref(),
            format!("{:width$}", "Ab", width = TAG_COL_WIDTH),
            "short tag must pad to fixed tag column"
        );
        // level badge then a plain gap (no badge fill) before the tag column
        let contents: Vec<&str> = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        let level_idx = contents
            .iter()
            .position(|c| *c == " I ")
            .expect("level badge");
        assert_eq!(contents[level_idx + 1], " ", "gap after level badge");
        assert!(
            contents[level_idx + 2].starts_with("Ab"),
            "tag column follows the gap"
        );
    }

    #[test]
    fn test_render_entry_lines_truncates_long_tag_in_fixed_column() {
        let long = "A".repeat(TAG_COL_WIDTH + 8);
        let row =
            EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {long}   : msg")).unwrap();
        let lines = render_entry_lines(&row, &[], 200, 1, 1);
        let tag_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains('…'))
            .expect("truncated tag span");
        assert_eq!(
            UnicodeWidthStr::width(tag_span.content.as_ref()),
            TAG_COL_WIDTH
        );
        assert!(
            tag_span.content.as_ref().ends_with('…') || tag_span.content.as_ref().contains('…'),
            "long tag must use ellipsis"
        );
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("msg"), "message must still be visible");
        assert!(
            !text.contains(&long),
            "full long tag must not spill into the line"
        );
    }

    #[test]
    fn test_render_entry_lines_continuation_indent_matches_header_width() {
        let row = EntryRow::from_line(
            "04-02 10:00:00.000  1  1 I Short : this message is long enough that it must wrap across more than one physical line when the column width is narrow",
        )
        .unwrap();
        let area_width = 40;
        let lines = render_entry_lines(&row, &[], area_width, 1, 1);
        assert!(lines.len() > 1);
        let lineno_s = "1 ";
        let ts = "04-02 10:00:00.000 ";
        let level = " I ";
        let prefix_without_tag =
            lineno_s.chars().count() + ts.chars().count() + level.chars().count() + LEVEL_TAG_GAP;
        let tag_col = tag_col_for_area(area_width, prefix_without_tag);
        let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
        let cont = lines[1].spans[0].content.as_ref();
        assert!(
            cont.chars().all(|c| c == ' '),
            "continuation prefix should be spaces"
        );
        assert_eq!(cont.chars().count(), header_width);
    }

    #[test]
    fn test_fit_tag_column_pads_and_truncates() {
        let (short, end) = fit_tag_column("Ab", 6);
        assert_eq!(short, "Ab    ");
        assert_eq!(end, 2);
        let (long, end) = fit_tag_column("abcdefghij", 6);
        assert_eq!(long, "abcde…");
        assert_eq!(end, 5);
    }

    #[test]
    fn test_render_entry_lines_shows_lineno_without_pid_tid() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 I MyTag   : hello").unwrap();
        let lines = render_entry_lines(&row, &[], 200, 12, 3);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains(" 12 "),
            "lineno should be right-aligned to width 3"
        );
        assert!(text.contains("MyTag"));
        assert!(text.contains("hello"));
        assert!(
            !text.contains("1234"),
            "pid must not appear in default display"
        );
        assert!(
            !text.contains("5678"),
            "tid must not appear in default display"
        );
        assert!(text.contains(" I "), "level badge must remain");
    }

    #[test]
    fn test_render_entry_line_collapsed_truncates_msg_with_ellipsis() {
        let long = "word ".repeat(40);
        let line = format!("04-02 10:00:00.000  1  1 I Tag     : {long}");
        let row = EntryRow::from_line(&line).unwrap();
        let rendered = render_entry_line_collapsed(&row, &[], 60, 12, 3);
        let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('…'), "collapsed line must ellipsize: {text}");
        assert!(
            !text.contains('\n'),
            "collapsed line must stay a single physical line"
        );
        assert!(
            text.contains(" 12 "),
            "collapsed line must keep the lineno prefix like render_entry_lines"
        );
        assert!(
            text.starts_with(" 12 04-02 10:00:00.000"),
            "must start with lineno then timestamp"
        );
    }

    #[test]
    fn test_render_entry_line_collapsed_keeps_short_msg_untouched() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello").unwrap();
        let rendered = render_entry_line_collapsed(&row, &[], 200, 1, 1);
        let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.contains('…'), "short msg must not be truncated");
        assert!(text.contains("MyTag"));
        assert!(text.contains("hello"));
    }

    #[test]
    fn test_render_entry_line_collapsed_highlights_pattern() {
        let row =
            EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred").unwrap();
        let patterns = [("error", 0usize, true)];
        let rendered = render_entry_line_collapsed(&row, &patterns, 200, 1, 1);
        let matched = rendered
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "error")
            .expect("highlight span");
        assert_eq!(matched.style, theme::highlight_style_active(0));
    }

    #[test]
    fn test_render_log_list_collapsed_view_produces_single_line_items() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        let long = "word ".repeat(40);
        tx.send(
            crate::model::EntryRow::from_line(&format!(
                "04-02 10:00:00.000  1  1 I Tag     : {long}"
            ))
            .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.collapsed_view = true;

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut found_ellipsis = false;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].symbol() == "…" {
                    found_ellipsis = true;
                }
            }
        }
        assert!(
            found_ellipsis,
            "collapsed view must render truncated msg with ellipsis"
        );
    }

    #[test]
    fn test_chip_pill_and_highlight_pill_styles() {
        let (text, body) = theme::chip_pill_style(crate::input::ChipField::Tag, "MyTag", false);
        assert!(text.contains("MyTag"));
        assert_eq!(body.bg, Some(theme::accent()));
        let (_, disabled) = theme::chip_pill_style(crate::input::ChipField::Msg, "x", true);
        assert_eq!(disabled, theme::disabled_chip_style());
        let (_, search) = theme::highlight_pill_style("error", 0, false, false);
        assert_eq!(search, theme::highlight_style(0));
        let (_, active) = theme::highlight_pill_style("error", 0, false, true);
        assert_eq!(active, theme::highlight_style_active(0));
        assert!(active.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_render_input_box_hw_cursor_after_committed_pill() {
        use crate::input::{Chip, ChipField};

        let mut input = InputBox::default();
        input.chips.push(Chip {
            field: ChipField::Tag,
            value: "MyTag".into(),
        });
        // Continue typing after the pill — the historical bug skipped caret
        // entirely once chips were non-empty.
        input.draft = "x".into();

        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut cursor = None;
        terminal
            .draw(|frame| {
                cursor = render_input_box(&input, Mode::Insert, true, frame, frame.area());
                if let Some(pos) = cursor {
                    frame.set_cursor_position(pos);
                }
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let content = cell_text(buf);
        assert!(content.contains("MyTag"));
        assert!(content.contains('x'));
        let pos = cursor.expect("Insert mode must report a hardware cursor");
        // Cursor sits after draft char 'x' on the content row.
        assert!(pos.x > 0, "cursor x={pos:?}");
        assert_eq!(pos.y, 1);
    }

    #[test]
    fn test_chip_strip_selection_keeps_stable_layout() {
        use crate::filter_model::Group;
        use crate::input::{Chip, ChipField};

        let mut app = App::new(100);
        app.groups.groups.push(Group {
            label: "a".into(),
            chips: vec![Chip {
                field: ChipField::Tag,
                value: "A".into(),
            }],
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.groups.groups.push(Group {
            label: "b".into(),
            chips: vec![Chip {
                field: ChipField::Msg,
                value: "B".into(),
            }],
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.focus = Focus::ChipStrip;
        app.group_cursor = 0;

        // Single content row + outer rounded chrome = height 3.
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_chip_strip(&app, frame, frame.area()))
            .unwrap();
        let before = cell_text(terminal.backend().buffer());

        app.group_cursor = 1;
        terminal
            .draw(|frame| render_chip_strip(&app, frame, frame.area()))
            .unwrap();
        let after = cell_text(terminal.backend().buffer());

        // Selection only restyles the group dot — glyph layout stays put.
        assert!(before.contains('A') && after.contains('A'));
        assert!(before.contains('B') && after.contains('B'));
        assert_eq!(
            before.chars().filter(|c| *c == 'A' || *c == 'B').count(),
            after.chars().filter(|c| *c == 'A' || *c == 'B').count()
        );
        // divider_block draws top + bottom horizontal rules (─), no rounded corners.
        let rules = before.chars().filter(|c| *c == '─').count();
        assert!(
            rules >= 2,
            "strip should have top+bottom ─ rules, got {rules}"
        );
        let rounded = before.chars().filter(|c| "╭╮╰╯".contains(*c)).count();
        assert_eq!(
            rounded, 0,
            "Filter strip must stay divider-only (no rounded modal corners)"
        );
    }

    #[test]
    fn test_filter_strip_wraps_and_grows_height() {
        use crate::filter_model::Group;
        use crate::input::{Chip, ChipField};

        let mut app = App::new(100);
        for label in ["AAAA", "BBBB", "CCCC", "DDDD"] {
            app.groups.groups.push(Group {
                label: label.into(),
                chips: vec![Chip {
                    field: ChipField::Tag,
                    value: label.into(),
                }],
                enabled: true,
                same_field_op: crate::fuzzy::SameFieldOp::And,
            });
        }
        let h = filter_strip_height(&app, 20);
        assert!(
            h > 3,
            "wrapped strip should exceed one content row + chrome, got {h}"
        );
        assert_eq!(
            filter_strip_height(&app, 20),
            h,
            "height is instantaneous (stable)"
        );
        app.groups.groups.clear();
        assert_eq!(filter_strip_height(&app, 20), 0);
    }

    #[test]
    fn test_render_status_bar_shows_highlight_match_stats() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap())
            .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : hit one").unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I T   : hit two").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());

        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("-/2"),
            "cursor not on hit: got {content:?}"
        );
        assert!(
            !content.contains("[-/2]"),
            "match stats must not use brackets: got {content:?}"
        );

        app.cursor = 1;
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("1/2"),
            "first hit ordinal: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_shows_disconnect_icon_in_stream_mode() {
        let mut app = App::new(100);
        app.following = false;
        app.ingest_done = true;
        app.export_source = crate::export::ExportSource::Hdc { device: None };

        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains(theme::GLYPH_DISCONNECT),
            "stream mode + ingest_done should show disconnect icon: got {content:?}"
        );
        assert!(
            !content.contains(theme::GLYPH_SOURCE_HDC),
            "disconnect must hide source glyph: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_shows_source_icon_when_live_connected() {
        let mut app = App::new(100);
        app.following = false;
        app.ingest_done = false;
        app.export_source = crate::export::ExportSource::Adb { device: None };

        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains(theme::GLYPH_SOURCE_ADB),
            "connected live should show source glyph: got {content:?}"
        );
        assert!(
            !content.contains(theme::GLYPH_DISCONNECT),
            "connected live must not show disconnect: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_hides_disconnect_icon_in_file_mode() {
        use crate::store::FileStore;
        use std::io::Write;

        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"04-02 10:00:00.000  1  1 I Tag     : ok\n")
            .unwrap();
        f.flush().unwrap();
        let mut app = App::new(100);
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.following = false;
        app.ingest_done = true;

        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            !content.contains(theme::GLYPH_DISCONNECT),
            "file mode must never show disconnect icon: got {content:?}"
        );
        assert!(
            content.contains(theme::GLYPH_SOURCE_FILE),
            "file mode should show source file glyph: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_shows_context_help_when_wide() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("help") && content.contains("filter"),
            "wide idle bar should show curated hints: got {content:?}"
        );
        assert!(
            !content.contains("j/k"),
            "idle LogList must not dump full L1: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_hides_context_help_when_narrow() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;

        // Wide enough for cursor + follow + source, too tight for help (avail < 8).
        let backend = TestBackend::new(12, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains(theme::GLYPH_FOLLOWING),
            "follow icon must win over help: got {content:?}"
        );
        assert!(
            !content.contains("help") && !content.contains("j/k"),
            "narrow bar should hide help entirely: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_follow_visible_when_paused() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;

        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains(theme::GLYPH_FOLLOWING),
            "paused follow must still occupy a slot: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_pending_chip_has_fields_not_prefix() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;
        app.pending_chip = true;

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            !content.contains("c…"),
            "left cluster must not show pending prefix: got {content:?}"
        );
        assert!(
            content.contains("tag") && content.contains("msg"),
            "pending L2 must list chip fields: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_flash_pill_with_pending_l2() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;
        app.pending_chip = true;
        app.set_flash("EXISTS");

        let backend = TestBackend::new(100, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let content = cell_text(buf);
        assert!(
            content.contains("EXISTS"),
            "flash pill must stay visible with pending L2: got {content:?}"
        );
        assert!(
            content.contains("tag"),
            "pending L2 must not cover the flash slot: got {content:?}"
        );
        assert_eq!(
            row0_bg_at_needle(buf, "EXISTS"),
            Some(theme::success()),
            "EXISTS flash must be a filled success pill: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_narrow_hides_hints_keeps_flash_floor() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;
        app.set_flash("EXISTS");

        // Wide enough for left icons + natural flash, too tight for FLASH_MIN
        // reservation + MIN_HELP_WIDTH — hints must hide first.
        let backend = TestBackend::new(24, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("EXISTS"),
            "flash floor must survive a tight row: got {content:?}"
        );
        assert!(
            !content.contains("help") && !content.contains("filter"),
            "hints must hide before the flash floor is eaten: got {content:?}"
        );
        assert!(
            content.contains(theme::GLYPH_FOLLOWING),
            "left follow slot must never yield: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_failed_flash_uses_warning_fill() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;
        app.set_flash("YANK FAILED");

        let backend = TestBackend::new(60, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let content = cell_text(buf);
        assert!(
            content.contains("YANK FAILED"),
            "failed flash copy must render: got {content:?}"
        );
        assert_eq!(
            row0_bg_at_needle(buf, "YANK FAILED"),
            Some(theme::warning()),
            "FAILED flash must use warning fill: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_strip_idle_is_help_and_del() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::ChipStrip;

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("help") && content.contains("del"),
            "strip idle should show help + del: got {content:?}"
        );
        assert!(
            !content.contains("group"),
            "strip idle must not dump full L1: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_picker_expands_full_hints() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;
        app.open_picker(crate::picker::PickerKind::Filter);

        let backend = TestBackend::new(100, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("select") || content.contains("close"),
            "picker must expand full context hints: got {content:?}"
        );
        assert!(
            !content.contains("? help") && !content.contains("?help"),
            "picker must not keep idle LogList 1–2: got {content:?}"
        );
    }

    #[test]
    fn test_minimap_row_maps_ends() {
        assert_eq!(minimap_row_for_index(0, 100, 10), 0);
        assert_eq!(minimap_row_for_index(99, 100, 10), 9);
        assert_eq!(minimap_row_for_index(50, 100, 10), 4);
    }

    #[test]
    fn test_build_minimap_marks_severe_and_search() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        for (i, level, msg) in [
            (0, "I", "ok"),
            (1, "E", "boom"),
            (2, "I", "findme here"),
            (3, "I", "ok"),
        ] {
            let _ = i;
            tx.send(
                EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 {level} Tag     : {msg}"))
                    .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("findme").unwrap());
        app.list_offset = 0;

        let marks = build_minimap_marks(&app, 4);
        assert_eq!(marks.len(), 4);
        assert!(marks.iter().any(|m| *m == MinimapMark::Severe));
        assert!(marks.iter().any(|m| *m == MinimapMark::Highlight));
        assert!(marks.iter().any(|m| *m == MinimapMark::Viewport));
        // Index 1 (E) maps near row 1 of 4.
        assert_eq!(marks[minimap_row_for_index(1, 4, 4)], MinimapMark::Severe);
    }

    #[test]
    fn test_build_minimap_marks_file_uses_cache_and_hit_index() {
        use crate::store::FileStore;
        use std::io::Write;

        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(
            b"04-02 10:00:00.000  1  1 I Tag     : ok\n\
              04-02 10:00:01.000  1  1 E Tag     : boom\n\
              04-02 10:00:02.000  1  1 I Tag     : findme here\n\
              04-02 10:00:03.000  1  1 I Tag     : ok\n",
        )
        .unwrap();
        f.flush().unwrap();
        let mut app = App::new(100);
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        // Wait for severe prefetch so cache is warm (no UI row_at needed).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let done = app
                .store
                .as_file()
                .map(|fs| fs.progress().severe_done)
                .unwrap_or(true);
            if done {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("severe prefetch timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.highlight_scan.hits = vec![2];
        app.highlight_scan.done = true;
        app.list_offset = 0;

        let marks = build_minimap_marks(&app, 4);
        assert!(marks.iter().any(|m| *m == MinimapMark::Severe));
        assert!(marks.iter().any(|m| *m == MinimapMark::Highlight));
        assert_eq!(marks[minimap_row_for_index(1, 4, 4)], MinimapMark::Severe);
        assert_eq!(
            marks[minimap_row_for_index(2, 4, 4)],
            MinimapMark::Highlight
        );
    }

    #[test]
    fn test_build_minimap_marks_bookmark_over_highlight() {
        // F5: a bookmarked alive row produces a Bookmark mark; on overlap with
        // a Highlight mark, Bookmark wins (Severe > Bookmark > Highlight).
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : findme").unwrap())
            .unwrap();
        tx.send(EntryRow::from_line("04-02 10:00:01.000  1  1 I Tag     : other").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("findme").unwrap());
        app.cursor = 0;
        app.bookmark_add_current();
        app.list_offset = 0;

        let marks = build_minimap_marks(&app, 4);
        assert!(
            marks.iter().any(|m| *m == MinimapMark::Bookmark),
            "bookmark row yields a Bookmark mark: {marks:?}"
        );
        // Index 0 is both highlighted and bookmarked; Bookmark must win there.
        assert_eq!(marks[minimap_row_for_index(0, 2, 4)], MinimapMark::Bookmark);
    }

    #[test]
    fn test_build_minimap_empty_when_no_visible() {
        let app = App::new(100);
        assert!(build_minimap_marks(&app, 10).is_empty());
    }

    #[test]
    fn test_render_log_list_draws_minimap_rail() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 E Tag     : err").unwrap())
            .unwrap();
        for i in 0..8 {
            tx.send(
                EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}"))
                    .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);

        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        let buf = terminal.backend().buffer();
        // Inner rightmost content col (width 40 → border at 0/39, inner 1..38, rail at 38).
        let rail_x = buf.area.width - 2;
        let mut found_mark = false;
        for y in 1..buf.area.height.saturating_sub(1) {
            let ch = buf[(rail_x, y)].symbol();
            if ch == "•" || ch == "│" {
                found_mark = true;
                break;
            }
        }
        assert!(
            found_mark,
            "minimap rail should paint │/• inside the log border"
        );
    }

    #[test]
    fn split_picker_lr_respects_ratio() {
        let area = Rect::new(0, 0, 100, 40);
        let (l, r) = split_picker_lr(area, 0.4);
        assert_eq!(l.width, 40);
        assert_eq!(r.width, 60);
        assert_eq!(l.height, r.height);
    }

    #[test]
    fn picker_left_stack_search_at_bottom() {
        let left = Rect::new(0, 0, 40, 20);
        let (cand, search) = picker_left_stack(left, 0);
        assert_eq!(search.height, 3);
        assert_eq!(search.y + search.height, left.y + left.height);
        assert_eq!(cand.y, left.y);
        assert_eq!(cand.height, left.height - 3);

        let (cand, search) = picker_left_stack(left, 1);
        assert_eq!(search.height, 4);
        assert_eq!(search.y + search.height, left.y + left.height);
        assert_eq!(cand.height, left.height - 4);

        let (cand, search) = picker_left_stack(left, 2);
        assert_eq!(search.height, 5);
        assert_eq!(cand.height, left.height - 5);
    }

    #[test]
    fn editable_text_spans_follow_caret_when_truncated() {
        let (spans, caret_col) = editable_text_spans("abcdefghij", 10, Some(5));
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            joined.contains('j'),
            "end char must remain visible: {joined:?}"
        );
        assert!(
            !joined.contains('a'),
            "start char should scroll off: {joined:?}"
        );
        assert_eq!(caret_col as usize, UnicodeWidthStr::width(joined.as_str()));
    }

    #[test]
    fn editable_text_spans_mid_caret_is_plain_text() {
        let (spans, caret_col) = editable_text_spans("abcdef", 2, None);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "abcdef");
        assert_eq!(caret_col, 2);
        assert!(!joined.contains('▏'));
    }

    #[test]
    fn editable_text_spans_start_caret_col_is_zero() {
        let (spans, caret_col) = editable_text_spans("ab", 0, None);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "ab");
        assert_eq!(caret_col, 0);
    }

    #[test]
    fn picker_search_line_has_rounded_border_padding_and_icon_gap() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut cursor = None;
        terminal
            .draw(|frame| {
                cursor = render_picker_search_line(
                    &crate::picker::PickerMode::Manage,
                    "abc",
                    3,
                    &[],
                    false,
                    None,
                    None,
                    0,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), "╭");
        assert_eq!(buf[(19, 0)].symbol(), "╮");
        assert_eq!(buf[(0, 2)].symbol(), "╰");
        assert_eq!(buf[(19, 2)].symbol(), "╯");
        assert_eq!(buf[(1, 1)].symbol(), " ");
        assert_eq!(buf[(2, 1)].symbol(), theme::GLYPH_MODE_MANAGE);
        assert_eq!(buf[(3, 1)].symbol(), " ");
        assert_eq!(buf[(4, 1)].symbol(), " ");
        assert_eq!(buf[(5, 1)].symbol(), "a");
        assert_eq!(cursor, Some(Position { x: 8, y: 1 }));
    }

    #[test]
    fn picker_search_line_cursor_after_last_char_when_full() {
        // area 20: border 2 → inner 18 → pad 1+1 → content 16; prefix icon+"  " = 3
        // → draft budget 13. Fill with 13 chars at end caret.
        let text = "abcdefghijklm"; // 13
        assert_eq!(text.len(), 13);
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut cursor = None;
        terminal
            .draw(|frame| {
                cursor = render_picker_search_line(
                    &crate::picker::PickerMode::Manage,
                    text,
                    text.chars().count(),
                    &[],
                    false,
                    None,
                    None,
                    0,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let pos = cursor.expect("hardware cursor");
        // content.x = 2; content.width = 16 → caret after last char at x = 18
        // (right padding), still left of the right border at x = 19.
        assert_eq!(pos.x, 18, "cursor should sit after last visible char");
        assert_eq!(pos.y, 1);
        let buf = terminal.backend().buffer();
        assert_eq!(buf[(19, 1)].symbol(), "│");
        // Last draft char 'm' occupies content col 15 → screen x = 2+15 = 17
        assert_eq!(buf[(17, 1)].symbol(), "m");
        assert!(
            pos.x > 17,
            "cursor must be to the right of last char, got {:?}",
            pos
        );
    }

    #[test]
    fn picker_search_line_keeps_committed_chips_above_border() {
        use crate::input::{Chip, ChipField};

        let chips = vec![Chip {
            field: ChipField::Tag,
            value: "MyTag".into(),
        }];
        let backend = TestBackend::new(40, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_picker_search_line(
                    &crate::picker::PickerMode::Edit { index: 0 },
                    "",
                    0,
                    &chips,
                    false,
                    None,
                    None,
                    1,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let content = cell_text(buf);
        assert!(content.lines().next().unwrap_or_default().contains("MyTag"));
        assert_eq!(buf[(0, 1)].symbol(), "╭");
        assert_eq!(buf[(39, 3)].symbol(), "╯");
    }

    #[test]
    fn picker_committed_chips_wrap_and_squeeze_candidates() {
        use crate::input::{Chip, ChipField};

        // Narrow width forces each long pill onto its own row.
        let chips = vec![
            Chip {
                field: ChipField::Tag,
                value: "AAAAAAAA".into(),
            },
            Chip {
                field: ChipField::Msg,
                value: "BBBBBBBB".into(),
            },
        ];
        let width = 18u16;
        let left_h = 20u16;
        let rows = committed_chip_rows(&chips, false, width, left_h);
        assert!(rows >= 2, "expected wrap to >=2 rows, got {rows}");

        let left = Rect::new(0, 0, width, left_h);
        let (cand, search) = picker_left_stack(left, rows);
        assert_eq!(search.height, PICKER_SEARCH_HEIGHT + rows);
        assert_eq!(cand.height, left_h - search.height);
        assert!(cand.height >= 1);

        let backend = TestBackend::new(width, search.height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_picker_search_line(
                    &crate::picker::PickerMode::New,
                    "",
                    0,
                    &chips,
                    false,
                    None,
                    None,
                    rows,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("AAAAAAAA"),
            "first chip visible: {content}"
        );
        assert!(
            content.contains("BBBBBBBB"),
            "wrapped second chip must remain visible: {content}"
        );
        // Second chip should not share the first chip row when wrapped.
        let lines: Vec<&str> = content.lines().collect();
        assert!(
            lines.len() >= 2,
            "chip band + input need multiple rows: {lines:?}"
        );
        assert!(
            lines[0].contains("AAAAAAAA"),
            "row0 should hold first chip: {:?}",
            lines[0]
        );
        assert!(
            lines[1].contains("BBBBBBBB"),
            "row1 should hold second chip: {:?}",
            lines[1]
        );
    }

    #[test]
    fn picker_search_area_shows_committed_chip() {
        use crate::input::{Chip, ChipField};

        let chips = vec![Chip {
            field: ChipField::Tag,
            value: "MyTag".into(),
        }];
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_picker(
                    "Filter · Edit",
                    &crate::picker::PickerMode::Edit { index: 0 },
                    "",
                    0,
                    "",
                    &chips,
                    false,
                    None,
                    &[],
                    &[],
                    &[],
                    &[],
                    0,
                    "no items",
                    PickerRightPane::Hits(&[]),
                    0.4,
                    true,
                    None,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("MyTag"));
    }

    #[test]
    fn picker_search_area_shows_confirmed_draft_field() {
        use crate::input::ChipField;

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_picker(
                    "Filter · New",
                    &crate::picker::PickerMode::New,
                    "",
                    0,
                    "",
                    &[],
                    false,
                    Some(ChipField::Tag),
                    &[],
                    &[],
                    &[],
                    &[],
                    0,
                    "no items",
                    PickerRightPane::Hits(&[]),
                    0.4,
                    true,
                    None,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("tag:"),
            "confirmed field must appear as tag: prefix, got: {content:?}"
        );
    }

    #[test]
    fn confirm_dialog_renders_delete_one_copy_over_picker() {
        use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

        let confirm = ConfirmKind::DeleteMany {
            items: vec![UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 0,
            }],
        };
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_picker(
                    "Manage",
                    &crate::picker::PickerMode::Manage,
                    "",
                    0,
                    "",
                    &[],
                    false,
                    None,
                    &["error".into()],
                    &[theme::muted()],
                    &[],
                    &[],
                    0,
                    "no items",
                    PickerRightPane::Hits(&[]),
                    0.4,
                    true,
                    None,
                    frame,
                    frame.area(),
                );
                render_confirm_dialog(&confirm, frame, frame.area());
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert_eq!(confirm_dialog_question(&confirm), "Delete selected?");
        assert!(content.contains("y/Enter"));
        assert!(content.contains("n/Esc"));
    }

    #[test]
    fn bookmark_summary_is_one_line_not_wrapped_bodies() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        let long_msg = "word ".repeat(20);
        tx.send(
            crate::model::EntryRow::from_line(&format!(
                "04-02 10:00:00.000  1  1 I Tag     : {long_msg}"
            ))
            .unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:08.000  1  1 I Tag     : later")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 0;
        app.bookmark_add_current();
        app.cursor = 1;
        app.bookmark_add_current();
        let line = app.bookmarks.summary_line();
        assert_eq!(line, "★ 2  10:00:00→10:00:08");
        let painted = bookmark_summary_line(&line);
        assert_eq!(painted.spans.len(), 1);
        assert!(!line.contains("word"), "summary must not embed pin bodies");
    }

    #[test]
    fn compare_panel_paints_pid_and_ignores_collapsed_view() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  42  99 I Tag     : hello")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 0;
        app.bookmark_add_current();
        app.collapsed_view = true;
        app.open_compare_panel();
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_compare_panel(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("Bookmark Compare"), "{content}");
        assert!(content.contains("42"), "pid must render: {content}");
        assert!(content.contains("99"), "tid must render: {content}");
        assert!(content.contains("hello"), "{content}");
    }

    #[test]
    fn render_picker_detail_shows_fields() {
        let row =
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA    : hello detail")
                .unwrap();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_picker_detail(Some(&row), frame, frame.area());
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("Detail"));
        assert!(content.contains("TagA"));
        assert!(content.contains("hello detail"));
    }

    #[test]
    fn render_picker_detail_stale_placeholder() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_picker_detail(None, frame, frame.area());
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("Detail") && content.contains("row gone"),
            "stale detail must show placeholder, got: {content:?}"
        );
    }

    #[test]
    fn confirm_dialog_renders_delete_many_count() {
        use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

        let confirm = ConfirmKind::DeleteMany {
            items: (0..12)
                .map(|i| UnifiedId {
                    kind: UnifiedKind::Filter,
                    source_index: i,
                })
                .collect(),
        };
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_confirm_dialog(&confirm, frame, frame.area());
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert_eq!(confirm_dialog_question(&confirm), "Delete 12 items?");
        assert!(content.contains("12"));
    }

    #[test]
    fn picker_frame_rect_compact_when_no_preview() {
        let frame = Rect::new(0, 0, 100, 40);
        let full = picker_frame_rect(frame, true);
        let compact = picker_frame_rect(frame, false);
        assert_eq!(compact.width, full.width / 2);
        assert_eq!(compact.height, full.height / 2);
        assert_eq!(
            compact.x,
            frame.x + (frame.width.saturating_sub(compact.width)) / 2
        );
        assert_eq!(
            compact.y,
            frame.y + (frame.height.saturating_sub(compact.height)) / 2
        );
    }

    #[test]
    fn preview_popup_rect_fills_remaining_space() {
        let frame = Rect::new(0, 0, 80, 40);
        let anchor = Rect::new(10, 2, 40, 3);
        let prev = preview_popup_rect(anchor, frame);
        assert!(
            prev.height > 12,
            "should fill below modal, got {}",
            prev.height
        );
        assert_eq!(prev.y + prev.height, frame.y + frame.height);
    }

    #[test]
    fn picker_preview_capacity_uses_right_pane_inner() {
        let frame = Rect::new(0, 0, 100, 40);
        let cap = picker_preview_capacity(frame, PICKER_PREVIEW_LEFT_RATIO);
        let picker = picker_frame_rect(frame, true);
        let (_l, r) = split_picker_lr_gapped(picker, PICKER_PREVIEW_LEFT_RATIO);
        assert_eq!(cap, r.height.saturating_sub(2) as usize);
        assert!(
            cap > 10,
            "tall picker should expose more than old PREVIEW_LIMIT"
        );
    }

    #[test]
    fn picker_preview_inner_width_matches_right_pane() {
        let frame = Rect::new(0, 0, 120, 40);
        let w = picker_preview_inner_width(frame, PICKER_PREVIEW_LEFT_RATIO);
        let picker = picker_frame_rect(frame, true);
        let (_l, r) = split_picker_lr_gapped(picker, PICKER_PREVIEW_LEFT_RATIO);
        assert_eq!(w, r.width.saturating_sub(2));
        assert!(
            w > 40,
            "real preview pane must be wider than the old hardcode"
        );
    }

    #[test]
    fn preset_preview_uses_width_so_chips_stay_unwrapped() {
        use crate::preset::{Preset, PresetChip, PresetFilterGroup, PRESET_VERSION};

        let preset = Preset {
            version: PRESET_VERSION,
            name: "trace".into(),
            filters: vec![PresetFilterGroup {
                chips: vec![
                    PresetChip {
                        field: "tag".into(),
                        value: "NTKernel".into(),
                    },
                    PresetChip {
                        field: "msg".into(),
                        value: "OidbSvcTrpcTcp".into(),
                    },
                    PresetChip {
                        field: "msg".into(),
                        value: "trace=".into(),
                    },
                ],
            }],
            excludes: vec![],
            highlights: vec![],
        };
        let narrow = preset_preview_lines(&preset, 40);
        let wide = preset_preview_lines(&preset, 80);
        // Title "Filter" + wrapped chip rows. Narrow width forces an extra wrap.
        let narrow_chip_rows = narrow.len().saturating_sub(1);
        let wide_chip_rows = wide.len().saturating_sub(1);
        assert!(
            narrow_chip_rows > wide_chip_rows,
            "width=40 wraps earlier than width=80: narrow={narrow_chip_rows} wide={wide_chip_rows}"
        );
        assert_eq!(
            wide_chip_rows, 1,
            "three chips of the trace preset fit on one row at width=80"
        );
    }

    #[test]
    fn split_picker_lr_gapped_leaves_one_col() {
        let area = Rect::new(0, 0, 100, 40);
        let (l, r) = split_picker_lr_gapped(area, 0.4);
        assert_eq!(l.x + l.width + 1, r.x);
        assert_eq!(l.width + r.width + 1, area.width);
    }

    #[test]
    fn modal_shell_draws_rounded_corners() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(2, 1, 36, 5);
                let _ = render_modal_shell("Input", frame, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Rounded BorderType corners (ratatui): ╭ ╮ ╰ ╯
        let mut corners = 0u32;
        for y in 1..6u16 {
            for x in 2..38u16 {
                match buf[(x, y)].symbol() {
                    "╭" | "╮" | "╰" | "╯" => corners += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(corners, 4, "modal shell should paint four rounded corners");
    }

    #[test]
    fn confirm_dialog_centers_on_frame() {
        use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

        let confirm = ConfirmKind::DeleteMany {
            items: vec![UnifiedId {
                kind: UnifiedKind::Filter,
                source_index: 0,
            }],
        };
        let frame = Rect::new(0, 0, 100, 40);
        let width = 34.min(frame.width).max(1);
        let height = 5.min(frame.height).max(1);
        let area = centered_modal_rect(frame, width, height);
        assert_eq!(area.x, frame.x + (frame.width.saturating_sub(width)) / 2);
        assert_eq!(area.y, frame.y + (frame.height.saturating_sub(height)) / 2);

        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_confirm_dialog(&confirm, f, f.area());
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("y/Enter"));
        assert!(content.contains("n/Esc"));
    }

    #[test]
    fn confirm_dialog_clear_all_copy() {
        let confirm = crate::picker::ConfirmKind::ClearAll;
        assert_eq!(confirm_dialog_question(&confirm), "Clear all rules?");
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_confirm_dialog(&confirm, f, f.area());
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("Clear all rules?"), "{content:?}");
    }

    #[test]
    fn bar_line_proportional_fill_and_count_label() {
        let line = bar_line("E", 5, 10, 20, Style::default());
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains('█'),
            "partial bar should have some filled blocks"
        );
        assert!(
            text.contains('░'),
            "partial bar should have some empty blocks"
        );
        assert!(
            text.trim_end().ends_with("5"),
            "trailing count label: {text:?}"
        );

        let full = bar_line("E", 10, 10, 20, Style::default());
        let full_text: String = full.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !full_text.contains('░'),
            "max count should fill the whole bar"
        );

        let empty = bar_line("E", 0, 10, 20, Style::default());
        let empty_text: String = empty.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !empty_text.contains('█'),
            "zero count should have no filled blocks"
        );
    }

    fn fake_summary_report() -> alnav::summary::SummaryOutput {
        use std::collections::HashMap;
        let mut levels = HashMap::new();
        levels.insert('I', 3usize);
        levels.insert('E', 2usize);
        let mut tag_levels = HashMap::new();
        tag_levels.insert('I', 2usize);
        tag_levels.insert('E', 1usize);
        alnav::summary::SummaryOutput {
            total: 5,
            matched: 5,
            levels,
            top_tags: vec![alnav::summary::TagEntry {
                tag: "MyTag".into(),
                count: 3,
                levels: tag_levels,
            }],
            time_range: alnav::summary::TimeRange {
                first: "04-02 10:00:00.000".into(),
                last: "04-02 10:00:04.000".into(),
            },
            top_errors: vec![alnav::summary::ErrorEntry {
                pattern: "timeout after <N>ms".into(),
                count: 2,
                tag: "MyTag".into(),
                sample: "timeout after 100ms".into(),
            }],
            crashes: 1,
        }
    }

    #[test]
    fn render_summary_panel_loading_shows_placeholder() {
        use crate::app::SummaryView;
        let mut app = App::new(100);
        app.summary_view = SummaryView::Loading;
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, 60, 20);
                render_summary_panel(&app, f, area);
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("Summary"));
        assert!(content.contains("computing"));
    }

    #[test]
    fn render_summary_panel_ready_shows_sections() {
        use crate::app::SummaryView;
        let mut app = App::new(100);
        app.summary_view = SummaryView::Ready(fake_summary_report());
        let backend = TestBackend::new(60, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, 60, 30);
                render_summary_panel(&app, f, area);
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("Rows: 5"));
        assert!(content.contains("Crashes: 1"));
        assert!(content.contains("Levels"));
        assert!(content.contains("Top tags"));
        assert!(content.contains("MyTag"));
        assert!(content.contains("Top errors"));
        assert!(content.contains("timeout after"));
    }

    #[test]
    fn summary_content_row_count_matches_report_lines() {
        use crate::app::SummaryView;
        let mut app = App::new(100);
        assert_eq!(summary_content_row_count(&app), 1); // Closed clamps to >=1
        app.summary_view = SummaryView::Loading;
        assert_eq!(summary_content_row_count(&app), 1);
        let report = fake_summary_report();
        let expected = summary_report_lines(&report).len();
        app.summary_view = SummaryView::Ready(report);
        assert_eq!(summary_content_row_count(&app), expected);
    }

    #[test]
    fn command_palette_empty_query_hides_catalog() {
        let mut app = App::new(100);
        app.open_command_palette();
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let _ = render_command_palette(&app, f, f.area());
            })
            .unwrap();
        let text = cell_text(terminal.backend().buffer());
        assert!(
            text.contains("Command Palette"),
            "input shell title: {text:?}"
        );
        assert!(
            !text.contains("Add Filter"),
            "empty query must not list commands: {text:?}"
        );
    }

    #[test]
    fn command_palette_filter_query_shows_add_filter_and_key() {
        let mut app = App::new(100);
        app.open_command_palette();
        app.command_palette
            .as_mut()
            .unwrap()
            .query
            .set_text("filter");
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let _ = render_command_palette(&app, f, f.area());
            })
            .unwrap();
        let text = cell_text(terminal.backend().buffer());
        assert!(text.contains("Add Filter"), "hits: {text:?}");
        assert!(
            text.contains(';'),
            "Filter New key hint should appear: {text:?}"
        );
    }
}

#[cfg(test)]
mod crash_detail_tests {
    use super::*;
    use crate::store::FileStore;
    use std::io::Write;

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn joined(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn push_stream_line(app: &mut App, line: &str) {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line(line).unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
    }

    #[test]
    fn crash_context_file_mode_merges_stack_continuation() {
        let body = "04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main\n\
             java.lang.RuntimeException: boom\n\
             \tat com.example.Foo.bar(Foo.java:10)\n\
             \tat com.example.Foo.baz(Foo.java:20)\n\
             04-02 10:00:01.000  1  1 I Tag     : next entry\n";
        let f = write_temp(body);
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        app.cursor = 0;
        let row = app.current_row().unwrap();
        let (info, truncated) = crash_context_for_row(&app, &row).expect("crash detected");
        assert!(!truncated);
        assert!(matches!(info.crash_type, CrashType::FatalException));
        assert_eq!(info.headline, "FATAL EXCEPTION: main");
        assert_eq!(
            info.exception.as_deref(),
            Some("java.lang.RuntimeException")
        );
        assert_eq!(info.stack.len(), 2);
        assert!(info.stack[0].contains("Foo.bar"));
        assert!(info.stack[1].contains("Foo.baz"));

        let rendered = joined(&render_crash_detail_lines(&info, false, truncated, 60));
        assert!(rendered.contains("FATAL EXCEPTION"));
        assert!(rendered.contains("Foo.bar"));
        assert!(!rendered.contains("truncated"));
    }

    #[test]
    fn crash_context_file_mode_truncates_at_scan_limit() {
        let mut body =
            String::from("04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main\n");
        for i in 0..600 {
            body.push_str(&format!("junk continuation line {i}\n"));
        }
        let f = write_temp(&body);
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        app.cursor = 0;
        let row = app.current_row().unwrap();
        let (info, truncated) = crash_context_for_row(&app, &row).expect("crash detected");
        assert!(truncated, "scan past 500 continuation lines must truncate");

        let rendered = joined(&render_crash_detail_lines(&info, false, truncated, 60));
        assert!(rendered.contains("truncated"));
    }

    #[test]
    fn crash_context_stream_mode_single_line_has_no_stack() {
        let mut app = App::new(100);
        push_stream_line(
            &mut app,
            "04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main",
        );
        app.cursor = 0;
        let row = app.current_row().unwrap();
        let (info, truncated) = crash_context_for_row(&app, &row).expect("crash detected");
        assert!(!truncated);
        assert!(info.stack.is_empty());
        assert_eq!(info.headline, "FATAL EXCEPTION: main");

        let rendered = joined(&render_crash_detail_lines(&info, true, truncated, 60));
        assert!(rendered.contains("stream"));
    }

    #[test]
    fn crash_context_none_for_non_crash_signature() {
        let mut app = App::new(100);
        push_stream_line(&mut app, r#"04-02 10:00:00.000  1  1 E Tag     : {"a":1}"#);
        app.cursor = 0;
        let row = app.current_row().unwrap();
        assert!(crash_context_for_row(&app, &row).is_none());
    }

    #[test]
    fn crash_context_none_for_bare_continuation_line() {
        let body = "04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main\n\
             \tat com.example.Foo.bar(Foo.java:10)\n";
        let f = write_temp(body);
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        app.cursor = 1;
        let row = app.current_row().unwrap();
        assert!(!row.is_parsed());
        assert!(crash_context_for_row(&app, &row).is_none());
    }
}
