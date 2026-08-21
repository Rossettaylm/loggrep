//! Single source of truth for alnav's color mapping (see CLAUDE.md
//! "UI 设计指导" for the design rules this module implements). Chrome and
//! log-severity tokens are mapped from a [`crate::palette::Palette`] via
//! [`map_to_tokens`]; style helpers read those tokens, not a separate log palette.
//!
//! UI chrome tokens (accent, selection, preview, …) and `[palette]` / `highlight`
//! overlays may be applied at startup via `theme.toml` (M4).

use std::sync::Mutex;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::Deserialize;

use alnav::parser::Level;

use crate::input::ChipField;
use crate::palette::{contrast_fg, mix, Palette};

// ---------------------------------------------------------------------------
// Nerdfont semantic glyphs (hard dependency — no runtime fallback).
// All UI iconography must reference these constants; `ui.rs` MUST NOT inline
// glyph literals. See prd.md R4 / design.md D1 for the rationale and table.
// ---------------------------------------------------------------------------

/// Manage/search prompt (Create / Bookmark / Unified). nf-fa-search — was
/// `\u{f0b7}` (fa-circle) which rendered cramped against the draft text.
pub const GLYPH_MODE_MANAGE: &str = "\u{f002}"; // nf-fa-search
pub const GLYPH_MODE_NEW: &str = "\u{f0fe}"; // nf-fa-plus_square
pub const GLYPH_MODE_EDIT: &str = "\u{f044}"; // nf-fa-pencil
pub const GLYPH_CARET_SEL: &str = "\u{f0da}"; //
pub const GLYPH_TITLE_PICKER: &str = "\u{f002}"; //
pub const GLYPH_TITLE_LOG: &str = "\u{f0c5}"; //
pub const GLYPH_TITLE_FILTER: &str = "\u{f0b0}"; //
pub const GLYPH_TITLE_EXCLUDE: &str = "\u{f056}"; //
pub const GLYPH_TITLE_HIGHLIGHT: &str = "\u{f0e0}"; //
pub const GLYPH_GROUP_ON: &str = "\u{f192}"; //
pub const GLYPH_GROUP_OFF: &str = "\u{f10c}"; //
pub const GLYPH_BOOKMARK: &str = "\u{f02e}"; // nf-fa-bookmark
/// Log-top compare-tray summary and panel stale mark (not the nerdfont pin).
pub const GLYPH_BOOKMARK_PIN: &str = "★";
pub const GLYPH_BOOKMARK_STALE_MARK: &str = "☆";
pub const GLYPH_COMPARE_UNTIMED: &str = "—";
pub const GLYPH_ACTION_JUMP: &str = "\u{f061}"; //  nf-fa-arrow_right
pub const GLYPH_ACTION_TOGGLE_ON: &str = "\u{f205}"; //  nf-fa-toggle_on
pub const GLYPH_ACTION_TOGGLE_OFF: &str = "\u{f204}"; //  nf-fa-toggle_off
pub const GLYPH_LOCK: &str = "\u{f023}"; //
pub const GLYPH_DISCONNECT: &str = "\u{f1e6}"; // nf-fa-plug (f127 chain-broken overflows right in non-Mono NF)
pub const GLYPH_TIME: &str = "\u{f017}"; // nf-fa-clock_o
pub const GLYPH_VIEW_FOCUS: &str = "\u{f06e}"; // nf-fa-eye
pub const GLYPH_FOLLOWING: &str = "\u{f062}"; //
pub const GLYPH_VISUAL: &str = "\u{f245}"; //
pub const GLYPH_SEARCH: &str = "\u{f002}"; //
pub const GLYPH_CRASH: &str = "\u{f071}"; //
pub const GLYPH_SEP: &str = "\u{e0bf}"; //
pub const GLYPH_FIELD_TAG: &str = "\u{f02b}"; //
pub const GLYPH_FIELD_MSG: &str = "\u{f075}"; //
pub const GLYPH_FIELD_PKG: &str = "\u{f187}"; //
pub const GLYPH_FIELD_PID: &str = "\u{f292}"; //
pub const GLYPH_FIELD_TID: &str = "\u{f2bd}"; //
pub const GLYPH_FIELD_LEVEL: &str = "\u{f0d0}"; //
pub const GLYPH_HR: &str = "\u{2500}"; // ─
pub const GLYPH_HELP: &str = "\u{f059}"; // nf-fa-question_circle
pub const GLYPH_PROGRESS: &str = "\u{f110}"; // nf-fa-spinner
pub const GLYPH_SOURCE_HDC: &str = "\u{f10b}"; // nf-fa-mobile
pub const GLYPH_SOURCE_ADB: &str = "\u{f17b}"; // nf-fa-android
pub const GLYPH_SOURCE_OPEN_FILE: &str = "\u{f07c}"; // nf-fa-folder_open
pub const GLYPH_SOURCE_RECENT: &str = "\u{f1da}"; // nf-fa-history
pub const GLYPH_SOURCE_FILE: &str = "\u{f15b}"; // nf-fa-file
pub const GLYPH_SOURCE_DIR: &str = "\u{f07b}"; // nf-fa-folder
pub const GLYPH_TITLE_DASHBOARD: &str = "\u{f0e4}"; // nf-fa-tachometer
pub const GLYPH_TITLE_PALETTE: &str = "\u{f0ca}"; // nf-fa-list-ul (command palette)
pub const GLYPH_QUIT: &str = "\u{f08b}"; // nf-fa-sign_out

/// Six-line dashboard-nvim-style Unicode wordmark. Keeping this in the theme
/// module makes the startup branding a single visual asset alongside semantic
/// glyphs.
pub const DASHBOARD_LOGO_WIDTH: u16 = 43;
pub const DASHBOARD_LOGO: [&str; 6] = [
    " █████╗ ██╗     ███╗   ██╗ █████╗ ██╗   ██╗",
    "██╔══██╗██║     ████╗  ██║██╔══██╗██║   ██║",
    "███████║██║     ██╔██╗ ██║███████║██║   ██║",
    "██╔══██║██║     ██║╚██╗██║██╔══██║╚██╗ ██╔╝",
    "██║  ██║███████╗██║ ╚████║██║  ██║ ╚████╔╝ ",
    "╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝╚═╝  ╚═╝  ╚═══╝  ",
];

/// Map a chip field to its nerdfont icon glyph.
pub fn field_icon(field: ChipField) -> &'static str {
    match field {
        ChipField::Tag => GLYPH_FIELD_TAG,
        ChipField::Msg => GLYPH_FIELD_MSG,
        ChipField::Pkg => GLYPH_FIELD_PKG,
        ChipField::Pid => GLYPH_FIELD_PID,
        ChipField::Tid => GLYPH_FIELD_TID,
        ChipField::Level => GLYPH_FIELD_LEVEL,
    }
}

/// UI chrome + palette-mapped log tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct UiTokens {
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub lock: Color,
    pub selection_frame: Color,
    pub log_selection_bg: Color,
    pub log_visual_bg: Color,
    pub preview_highlight_bg: Color,
    pub border_inactive: Color,
    /// Selected candidate row background (picker / field popup).
    pub candidate_selected_bg: Color,
    /// Selected candidate row text color.
    pub candidate_selected_fg: Color,
    /// Unselected candidate row background (`Reset` = inherit terminal).
    pub candidate_unselected_bg: Color,
    /// Unselected candidate row text color.
    pub candidate_unselected_fg: Color,
    /// Substring match characters inside candidate labels.
    pub candidate_match_fg: Color,
    /// Prefix drawn before the selected candidate row (e.g. `"▌ "`).
    pub candidate_prefix: String,
    pub bookmark_strip_bg: Color,
    /// Bookmark row background in LogList (faint yellow, distinct from selection).
    pub bookmark_row_bg: Color,
    pub canvas_bg: Color,
    pub canvas_fg: Color,
    pub muted: Color,
    pub error: Color,
    pub pkg: Color,
    pub pid: Color,
    pub tid: Color,
    pub highlight: [Color; 8],
    pub level_v: Color,
    pub level_d: Color,
    pub level_i: Color,
    pub level_w: Color,
    pub level_e: Color,
    pub level_f: Color,
    /// Per-line colors for the six-row Dashboard Unicode wordmark.
    pub logo: [Color; 6],
}

impl UiTokens {
    pub fn builtin() -> Self {
        map_to_tokens(&Palette::default_ansi())
    }
}

static TOKENS: Mutex<Option<UiTokens>> = Mutex::new(None);

/// Install tokens for the process (startup / tests).
pub fn install(tokens: UiTokens) {
    *TOKENS.lock().expect("theme lock") = Some(tokens);
}

fn t() -> UiTokens {
    TOKENS
        .lock()
        .expect("theme lock")
        .clone()
        .unwrap_or_else(UiTokens::builtin)
}

pub fn accent() -> Color {
    t().accent
}
pub fn success() -> Color {
    t().success
}
pub fn warning() -> Color {
    t().warning
}
pub fn lock() -> Color {
    t().lock
}
pub fn selection_frame() -> Color {
    t().selection_frame
}

pub fn map_to_tokens(p: &Palette) -> UiTokens {
    map_to_tokens_for(p, "default")
}

/// Map `p` through the fixed slot table, then apply the builtin signature
/// (accent + wordmark ramp) for `canonical`. Unknown names behave like `default`.
pub fn map_to_tokens_for(p: &Palette, canonical: &str) -> UiTokens {
    let canonical = crate::palette::resolve_theme_name(canonical).unwrap_or("default");
    let accent = signature_accent(p, canonical);
    let washes_from_mix = !matches!(p.background, Color::Reset);
    let mix_or = |tint: Color, t: u8, fallback: Color| -> Color {
        if washes_from_mix {
            mix(p.background, tint, t).unwrap_or(fallback)
        } else {
            fallback
        }
    };
    UiTokens {
        accent,
        success: p.green,
        warning: p.yellow,
        lock: p.magenta,
        selection_frame: p.magenta,
        log_selection_bg: mix_or(accent, 22, Color::DarkGray),
        log_visual_bg: mix_or(p.blue, 26, Color::Rgb(30, 60, 70)),
        preview_highlight_bg: mix_or(accent, 28, Color::DarkGray),
        border_inactive: p.bright_black,
        candidate_selected_bg: mix_or(p.foreground, 16, Color::DarkGray),
        candidate_selected_fg: if washes_from_mix {
            p.bright_white
        } else {
            Color::White
        },
        candidate_unselected_bg: if washes_from_mix {
            p.background
        } else {
            Color::Reset
        },
        candidate_unselected_fg: if washes_from_mix {
            p.white
        } else {
            Color::Gray
        },
        candidate_match_fg: accent,
        candidate_prefix: "▌ ".to_string(),
        bookmark_strip_bg: mix_or(p.yellow, 8, Color::DarkGray),
        bookmark_row_bg: mix_or(p.yellow, 12, Color::Rgb(54, 46, 0)),
        canvas_bg: p.background,
        canvas_fg: p.foreground,
        muted: p.bright_black,
        error: p.red,
        pkg: p.bright_yellow,
        pid: p.magenta,
        tid: p.bright_magenta,
        highlight: [
            p.yellow,
            p.bright_yellow,
            p.red,
            p.magenta,
            p.blue,
            p.cyan,
            p.green,
            p.bright_green,
        ],
        level_v: p.bright_black,
        level_d: p.blue,
        level_i: p.green,
        level_w: p.yellow,
        level_e: p.red,
        level_f: p.bright_red,
        logo: logo_ramp(p, canonical, accent),
    }
}

fn signature_accent(p: &Palette, canonical: &str) -> Color {
    match canonical {
        "onedark" | "tokyo-night" | "kanagawa" => p.blue,
        "dracula" | "catppuccin-mocha" => p.magenta,
        "everforest" => p.green,
        "gruvbox-dark" => p.yellow,
        _ => p.cyan,
    }
}

fn logo_ramp(p: &Palette, canonical: &str, accent: Color) -> [Color; 6] {
    let (a, b, c) = match canonical {
        "onedark" => (p.blue, p.cyan, p.magenta),
        "dracula" => (p.cyan, p.magenta, p.blue),
        "everforest" => (p.green, p.yellow, p.cyan),
        "tokyo-night" => (p.blue, p.magenta, p.cyan),
        "catppuccin-mocha" => (p.blue, p.magenta, p.red),
        "gruvbox-dark" => (p.yellow, p.red, p.green),
        "nord" => (p.blue, p.cyan, p.bright_cyan),
        "kanagawa" => (p.blue, p.magenta, p.cyan),
        _ => return [accent; 6],
    };
    [
        a,
        mix(a, b, 40).unwrap_or(a),
        mix(a, b, 80).unwrap_or(b),
        b,
        mix(b, c, 50).unwrap_or(c),
        c,
    ]
}

pub fn canvas_bg() -> Color {
    t().canvas_bg
}

pub fn canvas_style() -> Style {
    let tk = t();
    let mut style = Style::default();
    if !matches!(tk.canvas_bg, Color::Reset) {
        style = style.bg(tk.canvas_bg);
    }
    if !matches!(tk.canvas_fg, Color::Reset) {
        style = style.fg(tk.canvas_fg);
    }
    style
}

/// Timestamp/pid/tid/separator tint from the active palette's muted slot.
pub fn muted() -> Style {
    Style::default().fg(t().muted)
}

/// Right-aligned key chord in the command palette candidate list.
pub fn palette_keyhint_style() -> Style {
    muted().add_modifier(Modifier::DIM)
}

/// Tag/message foreground for severe log rows (E/F or crash signature).
/// `emphatic` is Fatal and crash lines (bold); Error-level stays red without bold.
pub fn severe_entry_style(emphatic: bool) -> Style {
    let mut style = Style::default().fg(t().error);
    if emphatic {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// Colored level badge (e.g. `" E "` on a red background).
pub fn level_badge_style(level: Level) -> Style {
    let bg = match level {
        Level::V => t().level_v,
        Level::D => t().level_d,
        Level::I => t().level_i,
        Level::W => t().level_w,
        Level::E => t().level_e,
        Level::F => t().level_f,
    };
    let mut style = Style::default().fg(contrast_fg(bg)).bg(bg);
    if matches!(level, Level::F) {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// Foreground-only variant of [`level_badge_style`]'s color, for the summary
/// panel's per-level bar chart (bar glyphs, not badge text-on-bg).
pub fn level_bar_style(level: Level) -> Style {
    let bg = level_badge_style(level).bg.unwrap_or(Color::White);
    Style::default().fg(bg)
}

/// Bar-chart color for the summary panel's Top-tags section (single accent).
pub fn accent_bar_style() -> Style {
    Style::default().fg(accent())
}

/// One of the 8 palette highlight colors, cycled by index.
/// TUI search chips assign a progressive index per pattern.
pub fn highlight_style(idx: usize) -> Style {
    let tk = t();
    let bg = tk.highlight[idx % tk.highlight.len()];
    Style::default()
        .fg(contrast_fg(bg))
        .bg(bg)
        .add_modifier(Modifier::BOLD)
}

/// [`highlight_style`] plus underline for the globally active search pattern.
pub fn highlight_style_active(idx: usize) -> Style {
    highlight_style(idx).add_modifier(Modifier::UNDERLINED)
}

/// Soft-disabled chip/group label (`di`): dim gray, distinct from focus and
/// from normal labels.
pub fn disabled_chip_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM)
}

/// Status-bar search hit counter `[k/N]`: accent foreground only (no reverse
/// badge), so it reads as related-but-distinct from the dim filter `cursor/total`.
pub fn highlight_match_status_style() -> Style {
    Style::default().fg(accent())
}

/// Chip field -> accent color, shared by the input box, popup, and (once
/// committed) the chip strip so a field always reads the same color
/// everywhere it appears.
pub fn field_color(field: ChipField) -> Color {
    match field {
        ChipField::Tag => accent(),
        ChipField::Msg => success(),
        ChipField::Pkg => t().pkg,
        ChipField::Pid => t().pid,
        ChipField::Tid => t().tid,
        ChipField::Level => warning(),
    }
}

/// Selected candidate row (picker / field popup).
pub fn candidate_selected_style() -> Style {
    Style::default()
        .fg(t().candidate_selected_fg)
        .bg(t().candidate_selected_bg)
}

/// Unselected candidate row base style.
/// Named themes (painted canvas) use white + DIM; default stays Gray without DIM.
pub fn candidate_unselected_style() -> Style {
    let tk = t();
    let mut style = Style::default()
        .fg(tk.candidate_unselected_fg)
        .bg(tk.candidate_unselected_bg);
    if !matches!(tk.canvas_bg, Color::Reset) {
        style = style.add_modifier(Modifier::DIM);
    }
    style
}

/// Match-character foreground for candidate substring hits.
pub fn candidate_match_style(selected: bool) -> Style {
    let bg = if selected {
        t().candidate_selected_bg
    } else {
        t().candidate_unselected_bg
    };
    Style::default().fg(t().candidate_match_fg).bg(bg)
}

/// Prefix string for the selected candidate row (nerdfont caret-right glyph).
pub fn candidate_prefix() -> String {
    format!("{} ", GLYPH_CARET_SEL)
}

/// Backward-compatible alias for selected candidate style.
pub fn candidate_selection_style() -> Style {
    candidate_selected_style()
}

/// Compact / Minimal Dashboard wordmark (`"alnav"`): signature accent.
pub fn dashboard_header_style() -> Style {
    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
}

/// One row of the six-line Unicode wordmark.
pub fn dashboard_logo_line_style(row: usize) -> Style {
    Style::default()
        .fg(t().logo[row % 6])
        .add_modifier(Modifier::BOLD)
}

/// Dashboard product subtitle, empty-state copy, and footer.
/// Named (painted) themes use canvas foreground so copy tracks the scheme;
/// `default` keeps muted DarkGray.
pub fn dashboard_muted_style() -> Style {
    let tk = t();
    let fg = if matches!(tk.canvas_fg, Color::Reset) {
        tk.muted
    } else {
        tk.canvas_fg
    };
    Style::default().fg(fg).add_modifier(Modifier::DIM)
}

/// Borderless Dashboard section heading.
pub fn dashboard_section_style() -> Style {
    Style::default()
        .fg(accent())
        .add_modifier(Modifier::BOLD | Modifier::DIM)
}

/// Base style for a Dashboard action/recent row.
pub fn dashboard_item_style(selected: bool) -> Style {
    if selected {
        candidate_selected_style()
    } else {
        candidate_unselected_style()
    }
}

/// Dim secondary copy while preserving the selected row's soft background.
pub fn dashboard_description_style(selected: bool) -> Style {
    let bg = dashboard_item_style(selected).bg;
    dashboard_muted_style().bg(bg.unwrap_or(Color::Reset))
}

/// Right-aligned Dashboard shortcut badge.
pub fn dashboard_hotkey_style(selected: bool) -> Style {
    let bg = dashboard_item_style(selected).bg;
    Style::default()
        .fg(accent())
        .bg(bg.unwrap_or(Color::Reset))
        .add_modifier(Modifier::BOLD)
}

/// Dashboard-local transient failure text.
pub fn dashboard_flash_style() -> Style {
    Style::default().fg(warning()).add_modifier(Modifier::DIM)
}

/// Soft accent+DIM style for picker mode prefixes (no fill — distinct from chip pills).
pub fn picker_mode_style() -> Style {
    Style::default().fg(accent()).add_modifier(Modifier::DIM)
}

/// Soft prompt icon + two trailing spaces (gap before draft text).
pub fn picker_prompt_prefix(icon: &'static str) -> Span<'static> {
    Span::styled(format!("{icon}  "), picker_mode_style())
}

/// Mode prefix icon (nerdfont): Manage search, New plus-square, Edit pencil.
/// Bookmark panels pass [`GLYPH_BOOKMARK`] via [`picker_prompt_prefix`] instead.
pub fn picker_mode_prefix(mode: &crate::picker::PickerMode) -> Span<'static> {
    let icon = match mode {
        crate::picker::PickerMode::Manage => GLYPH_MODE_MANAGE,
        crate::picker::PickerMode::New => GLYPH_MODE_NEW,
        crate::picker::PickerMode::Edit { .. } => GLYPH_MODE_EDIT,
    };
    picker_prompt_prefix(icon)
}

/// Style for the group `●`/`○` marker (selected = selection_frame, else dim).
/// One cell wide so chip strips stay a single content row tall.
pub fn chip_group_border_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(selection_frame())
    } else {
        Style::default()
            .fg(t().border_inactive)
            .add_modifier(Modifier::DIM)
    }
}

/// Build a filter pill as a single space-filled span (field-colored bg) with
/// the field icon prefixing the value. No powerline ends (Q3: weakened chrome).
/// `disabled` collapses to a single dim span (same shape, dim style).
pub fn chip_pill_spans(field: ChipField, value: &str, disabled: bool) -> Vec<Span<'static>> {
    let icon = field_icon(field);
    if disabled {
        let text = format!(" {icon} {value} ");
        return vec![Span::styled(text, disabled_chip_style())];
    }
    let body_text = format!(" {icon} {value} ");
    let body_style = match field {
        ChipField::Level => {
            let level = match value.chars().next().unwrap_or('I').to_ascii_uppercase() {
                'V' => Level::V,
                'D' => Level::D,
                'I' => Level::I,
                'W' => Level::W,
                'E' => Level::E,
                'F' => Level::F,
                _ => Level::I,
            };
            level_badge_style(level)
        }
        other => Style::default()
            .fg(contrast_fg(field_color(other)))
            .bg(field_color(other))
            .add_modifier(Modifier::BOLD),
    };
    vec![Span::styled(body_text, body_style)]
}

/// Backward-compatible single-span pill (tests / callers that don't need
/// powerline ends). Returns body text + style only.
pub fn chip_pill_style(field: ChipField, value: &str, disabled: bool) -> (String, Style) {
    if disabled {
        return (format!(" {value} "), disabled_chip_style());
    }
    let icon = field_icon(field);
    let text = format!(" {icon} {value} ");
    let style = match field {
        ChipField::Level => {
            let level = match value.chars().next().unwrap_or('I').to_ascii_uppercase() {
                'V' => Level::V,
                'D' => Level::D,
                'I' => Level::I,
                'W' => Level::W,
                'E' => Level::E,
                'F' => Level::F,
                _ => Level::I,
            };
            level_badge_style(level)
        }
        other => Style::default()
            .fg(contrast_fg(field_color(other)))
            .bg(field_color(other))
            .add_modifier(Modifier::BOLD),
    };
    (text, style)
}

/// Exclude pill (H9): space-filled pill with a `!` prefix before the field icon.
pub fn exclude_pill_spans(field: ChipField, value: &str, disabled: bool) -> Vec<Span<'static>> {
    let icon = field_icon(field);
    if disabled {
        let text = format!(" !{icon} {value} ");
        return vec![Span::styled(text, disabled_chip_style())];
    }
    let body_text = format!(" !{icon} {value} ");
    let body_style = match field {
        ChipField::Level => {
            let level = match value.chars().next().unwrap_or('I').to_ascii_uppercase() {
                'V' => Level::V,
                'D' => Level::D,
                'I' => Level::I,
                'W' => Level::W,
                'E' => Level::E,
                'F' => Level::F,
                _ => Level::I,
            };
            level_badge_style(level)
        }
        other => Style::default()
            .fg(contrast_fg(field_color(other)))
            .bg(field_color(other))
            .add_modifier(Modifier::BOLD),
    };
    vec![Span::styled(body_text, body_style)]
}

/// Backward-compatible single-span exclude pill.
pub fn exclude_pill_style(field: ChipField, value: &str, disabled: bool) -> (String, Style) {
    let (inner, style) = chip_pill_style(field, value, disabled);
    (format!("!{inner}"), style)
}

/// Search/highlight pill as a single space-filled span.
/// `active_global` underlines the globally active (n/N) search chip.
pub fn highlight_pill_spans(
    pattern: &str,
    color_idx: usize,
    disabled: bool,
    active_global: bool,
) -> Vec<Span<'static>> {
    if disabled {
        let text = format!(" {pattern} ");
        return vec![Span::styled(text, disabled_chip_style())];
    }
    let style = if active_global {
        highlight_style_active(color_idx)
    } else {
        highlight_style(color_idx)
    };
    vec![Span::styled(format!(" {pattern} "), style)]
}

/// Backward-compatible single-span highlight pill.
pub fn highlight_pill_style(
    pattern: &str,
    color_idx: usize,
    disabled: bool,
    active_global: bool,
) -> (String, Style) {
    let text = format!(" {pattern} ");
    if disabled {
        return (text, disabled_chip_style());
    }
    let style = if active_global {
        highlight_style_active(color_idx)
    } else {
        highlight_style(color_idx)
    };
    (text, style)
}

/// Border color for a bordered region: dimmed accent when it currently has
/// keyboard focus (reduced from full-saturation accent per Q3 border-weakening),
/// dim gray otherwise.
pub fn border_style(active: bool) -> Style {
    if active {
        Style::default().fg(accent()).add_modifier(Modifier::DIM)
    } else {
        Style::default()
            .fg(t().border_inactive)
            .add_modifier(Modifier::DIM)
    }
}

/// Glyph for a numbered region, chosen by its Tab-cycle digit.
fn numbered_glyph(number: u8) -> &'static str {
    match number {
        1 => GLYPH_TITLE_FILTER,
        2 => GLYPH_TITLE_EXCLUDE,
        3 => GLYPH_TITLE_HIGHLIGHT,
        4 => GLYPH_TITLE_LOG,
        5 => GLYPH_TITLE_PICKER,
        _ => GLYPH_TITLE_PICKER,
    }
}

/// Border title for a numbered, Tab-cyclable region (Filter/Exclude/Highlight/Log/Input):
/// a nerdfont glyph + digit badge + label, styled by whether the region is
/// currently focused. No reverse-color block (Q3: weakened borders).
pub fn numbered_title(number: u8, label: &str, active: bool) -> Line<'static> {
    numbered_title_with_loading(number, label, active, None)
}

/// Like [`numbered_title`], with an optional dim loading suffix (LogList L1 banner).
pub fn numbered_title_with_loading(
    number: u8,
    label: &str,
    active: bool,
    loading: Option<&str>,
) -> Line<'static> {
    let glyph = numbered_glyph(number);
    let badge_style = if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    let label_style = if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    let mut spans = vec![
        Span::styled(format!(" {glyph} {number} "), badge_style),
        Span::styled(format!(" {label} "), label_style),
    ];
    if let Some(msg) = loading {
        spans.push(Span::styled(format!(" {msg} "), log_loading_style(active)));
    }
    Line::from(spans)
}

/// Style for LogList index/filter/highlight progress text in the block title.
pub fn log_loading_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::DIM | Modifier::ITALIC)
    } else {
        Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC)
    }
}

/// Border title for a region that isn't part of the numbered Tab cycle
/// (the search box, the field popup). Prepends a nerdfont glyph.
pub fn plain_title(glyph: &str, label: &str, active: bool) -> Line<'static> {
    let label_style = if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Line::from(Span::styled(format!(" {glyph} {label} "), label_style))
}

/// Selected-row background in the log list — a quiet, low-contrast gray
/// instead of a full reverse-video block. Applied via `ListItem::style` (not
/// `List::highlight_style`) so keyword highlight spans keep their own bg;
/// see `ui::render_log_list`.
pub fn log_selection_style() -> Style {
    Style::default().bg(t().log_selection_bg)
}

/// Background for rows inside a visual-line selection (`V` … `y`). Distinct
/// from `log_selection_style` so the range reads as a block, not a single
/// cursor highlight.
pub fn log_visual_style() -> Style {
    Style::default().bg(t().log_visual_bg)
}

/// Icon-only status marker (follow / visual). No word label.
pub fn status_icon(glyph: &str, fg: Color) -> Span<'static> {
    Span::styled(
        format!(" {glyph} "),
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    )
}

/// Icon + short value (lock / time / progress). Glyph carries the noun.
pub fn status_icon_value(glyph: &str, value: &str, fg: Color) -> Span<'static> {
    Span::styled(
        format!(" {glyph} {value} "),
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    )
}

/// Soft (non-inverse) pending / flash text. Kept for non-status-bar callers.
pub fn status_soft(text: &str, fg: Color) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::default().fg(fg).add_modifier(Modifier::DIM),
    )
}

fn status_pill_style(fg: Color) -> Style {
    Style::default()
        .fg(contrast_fg(fg))
        .bg(fg)
        .add_modifier(Modifier::BOLD)
}

/// Filled status pill (` {glyph} `). On-pill fg from [`contrast_fg`].
pub fn status_pill(glyph: &str, fg: Color) -> Span<'static> {
    Span::styled(format!(" {glyph} "), status_pill_style(fg))
}

/// Filled status pill with a short value (` {glyph} {value} `).
pub fn status_pill_value(glyph: &str, value: &str, fg: Color) -> Span<'static> {
    Span::styled(format!(" {glyph} {value} "), status_pill_style(fg))
}

/// Follow off-state: same slot shape as [`status_pill`], DIM, no fill.
pub fn status_icon_dim(glyph: &str) -> Span<'static> {
    Span::styled(
        format!(" {glyph} "),
        Style::default().add_modifier(Modifier::DIM),
    )
}

fn flash_fill(text: &str) -> Color {
    if text.contains("FAILED") {
        warning()
    } else {
        success()
    }
}

/// Filled flash toast. Warning fill when the copy contains `FAILED`.
pub fn status_flash_pill(text: &str) -> Span<'static> {
    status_flash_pill_fit(text, usize::MAX)
}

/// Truncate flash copy so the pill (` {text} `) fits `max_chars`.
pub fn status_flash_pill_fit(text: &str, max_chars: usize) -> Span<'static> {
    let fg = flash_fill(text);
    if max_chars == 0 {
        return Span::raw("");
    }
    let full = format!(" {text} ");
    let shown = if full.chars().count() <= max_chars {
        full
    } else {
        let inner = max_chars.saturating_sub(2);
        let body: String = text.chars().take(inner).collect();
        if body.is_empty() {
            " ".repeat(max_chars.min(2))
        } else {
            format!(" {body} ")
        }
    };
    Span::styled(shown, status_pill_style(fg))
}

/// Dim trailing keybinding hint on the status bar (H6 context help).
pub fn context_help_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Help panel section title (inactive).
pub fn help_section_style(active: bool) -> Style {
    if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}

/// Substring hit inside the Help panel (non-current).
pub fn help_search_hit_style() -> Style {
    highlight_style(0)
}

/// Current Help search hit (same ramp slot, underlined).
pub fn help_search_current_style() -> Style {
    highlight_style_active(0)
}

/// Faint search-hit highlight inside the H1 Preview window (distinct from
/// formal [`highlight_style`] chips).
pub fn preview_highlight_style() -> Style {
    Style::default()
        .fg(contrast_fg(t().preview_highlight_bg))
        .bg(t().preview_highlight_bg)
        .add_modifier(Modifier::DIM)
}

/// Dim style for Preview placeholder / empty state.
pub fn preview_placeholder_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// H4 Detail overlay: label for non-chip fields (`time`).
pub fn detail_label_style() -> Style {
    muted().add_modifier(Modifier::DIM)
}

/// H4 Detail overlay: chip-field name tint (matches pill / popup).
pub fn detail_field_label_style(field: ChipField) -> Style {
    Style::default()
        .fg(field_color(field))
        .add_modifier(Modifier::BOLD)
}

/// H3 minimap: empty track (always drawn when `visible` non-empty).
pub fn minimap_track_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM)
}

/// H3 minimap: approximate viewport band (fainter than marks).
pub fn minimap_viewport_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .bg(t().border_inactive)
        .add_modifier(Modifier::DIM)
}

/// H3 minimap: enabled search-hit mark.
pub fn minimap_highlight_style() -> Style {
    Style::default().fg(accent())
}

/// H3 minimap: severe (E/F/crash) mark — wins over search on overlap.
pub fn minimap_severe_style() -> Style {
    Style::default().fg(t().error).add_modifier(Modifier::BOLD)
}

/// M2 bookmark strip background (subtle wash vs log body).
pub fn bookmark_strip_style() -> Style {
    Style::default().bg(t().bookmark_strip_bg)
}

/// M2 bookmark strip / picker label.
pub fn bookmark_label_style() -> Style {
    Style::default().fg(warning()).add_modifier(Modifier::BOLD)
}
/// LogList row background for bookmarked rows (faint yellow). Priority:
/// `visual > bookmark-bg > cursor-selection` (see `ui::render_log_list`).
pub fn bookmark_row_style() -> Style {
    Style::default().bg(t().bookmark_row_bg)
}

/// Foreground color for the minimap Bookmark mark (F5). Same color family as
/// the bookmark row bg so the rail mark reads as related to the row wash.
pub fn bookmark_minimap_color() -> Color {
    t().bookmark_row_bg
}

/// M2 stale bookmark (evicted from ring buffer).
pub fn bookmark_stale_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
}

/// Compare-panel Δt prefix (`+1.2s` / `—`).
pub fn compare_delta_style() -> Style {
    muted().add_modifier(Modifier::DIM)
}

/// Unified Manage list: kind prefix / row tint by category.
pub fn unified_kind_style(kind: crate::picker::UnifiedKind) -> Style {
    use crate::picker::UnifiedKind;
    match kind {
        UnifiedKind::Filter => Style::default().fg(accent()),
        UnifiedKind::Highlight => Style::default().fg(t().highlight[0]),
        UnifiedKind::Exclude => Style::default().fg(warning()),
    }
}

/// Candidate-list prefix when the row is Tab multi-selected (checked).
pub fn candidate_checked_prefix_style() -> Style {
    Style::default().fg(lock()).add_modifier(Modifier::BOLD)
}

// ── theme.toml parsing (M4) ──────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct ThemeFile {
    accent: Option<String>,
    success: Option<String>,
    warning: Option<String>,
    lock: Option<String>,
    selection_frame: Option<String>,
    log_selection_bg: Option<String>,
    log_visual_bg: Option<String>,
    preview_highlight_bg: Option<String>,
    border_inactive: Option<String>,
    candidate_selected_bg: Option<String>,
    candidate_selected_fg: Option<String>,
    candidate_unselected_bg: Option<String>,
    candidate_unselected_fg: Option<String>,
    candidate_match_fg: Option<String>,
    candidate_prefix: Option<String>,
    /// Deprecated alias for [`Self::candidate_selected_bg`].
    candidate_selection_bg: Option<String>,
    bookmark_strip_bg: Option<String>,
    bookmark_row_bg: Option<String>,
    highlight: Option<Vec<String>>,
    palette: Option<PaletteFile>,
}

#[derive(Debug, Deserialize, Default)]
struct PaletteFile {
    background: Option<String>,
    foreground: Option<String>,
    black: Option<String>,
    red: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    blue: Option<String>,
    magenta: Option<String>,
    cyan: Option<String>,
    white: Option<String>,
    bright_black: Option<String>,
    bright_red: Option<String>,
    bright_green: Option<String>,
    bright_yellow: Option<String>,
    bright_blue: Option<String>,
    bright_magenta: Option<String>,
    bright_cyan: Option<String>,
    bright_white: Option<String>,
}

fn apply_palette_file(mut p: Palette, f: PaletteFile) -> Result<Palette, String> {
    let set = |slot: &mut Color, v: Option<String>| -> Result<(), String> {
        if let Some(s) = v {
            *slot = parse_color(&s)?;
        }
        Ok(())
    };
    set(&mut p.background, f.background)?;
    set(&mut p.foreground, f.foreground)?;
    set(&mut p.black, f.black)?;
    set(&mut p.red, f.red)?;
    set(&mut p.green, f.green)?;
    set(&mut p.yellow, f.yellow)?;
    set(&mut p.blue, f.blue)?;
    set(&mut p.magenta, f.magenta)?;
    set(&mut p.cyan, f.cyan)?;
    set(&mut p.white, f.white)?;
    set(&mut p.bright_black, f.bright_black)?;
    set(&mut p.bright_red, f.bright_red)?;
    set(&mut p.bright_green, f.bright_green)?;
    set(&mut p.bright_yellow, f.bright_yellow)?;
    set(&mut p.bright_blue, f.bright_blue)?;
    set(&mut p.bright_magenta, f.bright_magenta)?;
    set(&mut p.bright_cyan, f.bright_cyan)?;
    set(&mut p.bright_white, f.bright_white)?;
    Ok(p)
}

/// Merge `theme.toml` onto the default ANSI palette.
pub fn apply_overlay(base: Palette, text: &str) -> Result<UiTokens, String> {
    apply_overlay_for(base, "default", text)
}

/// Merge `theme.toml` semantic keys and `[palette]` onto a named base palette.
pub fn apply_overlay_for(base: Palette, canonical: &str, text: &str) -> Result<UiTokens, String> {
    let file: ThemeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    if let Some(ref hl) = file.highlight {
        if hl.len() != 8 {
            return Err(format!("highlight must have 8 colors, got {}", hl.len()));
        }
    }
    let pal = if let Some(pf) = file.palette {
        apply_palette_file(base, pf)?
    } else {
        base
    };
    let mut t = map_to_tokens_for(&pal, canonical);
    if let Some(s) = file.accent {
        t.accent = parse_color(&s)?;
    }
    if let Some(s) = file.success {
        t.success = parse_color(&s)?;
    }
    if let Some(s) = file.warning {
        t.warning = parse_color(&s)?;
    }
    if let Some(s) = file.lock {
        t.lock = parse_color(&s)?;
    }
    if let Some(s) = file.selection_frame {
        t.selection_frame = parse_color(&s)?;
    }
    if let Some(s) = file.log_selection_bg {
        t.log_selection_bg = parse_color(&s)?;
    }
    if let Some(s) = file.log_visual_bg {
        t.log_visual_bg = parse_color(&s)?;
    }
    if let Some(s) = file.preview_highlight_bg {
        t.preview_highlight_bg = parse_color(&s)?;
    }
    if let Some(s) = file.border_inactive {
        t.border_inactive = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_selected_bg {
        t.candidate_selected_bg = parse_color(&s)?;
    } else if let Some(s) = file.candidate_selection_bg {
        t.candidate_selected_bg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_selected_fg {
        t.candidate_selected_fg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_unselected_bg {
        t.candidate_unselected_bg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_unselected_fg {
        t.candidate_unselected_fg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_match_fg {
        t.candidate_match_fg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_prefix {
        t.candidate_prefix = s;
    }
    if let Some(s) = file.bookmark_strip_bg {
        t.bookmark_strip_bg = parse_color(&s)?;
    }
    if let Some(s) = file.bookmark_row_bg {
        t.bookmark_row_bg = parse_color(&s)?;
    }
    if let Some(hl) = file.highlight {
        let mut colors = [Color::Reset; 8];
        for (i, s) in hl.into_iter().enumerate() {
            colors[i] = parse_color(&s)?;
        }
        t.highlight = colors;
    }
    Ok(t)
}

/// Parse a theme.toml body into tokens (merged onto builtin ANSI defaults).
pub fn parse_theme_toml(text: &str) -> Result<UiTokens, String> {
    apply_overlay(Palette::default_ansi(), text)
}

/// Named ratatui color or `#RRGGBB` / `#RGB`.
pub fn parse_color(s: &str) -> Result<Color, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    match s.to_ascii_lowercase().as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "gray" | "grey" => Ok(Color::Gray),
        "darkgray" | "darkgrey" => Ok(Color::DarkGray),
        "lightred" => Ok(Color::LightRed),
        "lightgreen" => Ok(Color::LightGreen),
        "lightyellow" => Ok(Color::LightYellow),
        "lightblue" => Ok(Color::LightBlue),
        "lightmagenta" => Ok(Color::LightMagenta),
        "lightcyan" => Ok(Color::LightCyan),
        "white" => Ok(Color::White),
        "reset" => Ok(Color::Reset),
        other => Err(format!("unknown color '{other}'")),
    }
}

fn parse_hex(hex: &str) -> Result<Color, String> {
    let expand = |c: u8| -> u8 { c * 17 };
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).map_err(|e| e.to_string())?;
            let g = u8::from_str_radix(&hex[1..2], 16).map_err(|e| e.to_string())?;
            let b = u8::from_str_radix(&hex[2..3], 16).map_err(|e| e.to_string())?;
            Ok(Color::Rgb(expand(r), expand(g), expand(b)))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?;
            let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?;
            let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?;
            Ok(Color::Rgb(r, g, b))
        }
        _ => Err(format!("invalid hex color '#{hex}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Palette;

    #[test]
    fn test_log_selection_style_is_soft_gray_no_reverse() {
        install(UiTokens::builtin());
        let style = log_selection_style();
        assert_eq!(style.bg, Some(Color::DarkGray));
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn severe_entry_style_uses_theme_error_red() {
        install(UiTokens::builtin());
        let normal = severe_entry_style(false);
        assert_eq!(normal.fg, Some(Color::Red));
        assert!(!normal.add_modifier.contains(Modifier::BOLD));
        let emphatic = severe_entry_style(true);
        assert_eq!(emphatic.fg, Some(Color::Red));
        assert!(emphatic.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_candidate_selection_style_soft_gray_with_white_fg() {
        install(UiTokens::builtin());
        let style = candidate_selection_style();
        assert_eq!(style.bg, Some(Color::DarkGray));
        assert_eq!(style.fg, Some(Color::White));
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_candidate_match_and_prefix_tokens() {
        install(UiTokens::builtin());
        assert_eq!(candidate_match_style(false).fg, Some(Color::Cyan));
        assert_eq!(candidate_prefix(), format!("{} ", GLYPH_CARET_SEL));
        use crate::picker::PickerMode;
        assert_eq!(
            picker_mode_prefix(&PickerMode::Manage).content,
            format!("{}  ", GLYPH_MODE_MANAGE)
        );
        assert_eq!(
            picker_mode_prefix(&PickerMode::New).content,
            format!("{}  ", GLYPH_MODE_NEW)
        );
        assert_eq!(
            picker_mode_prefix(&PickerMode::Edit { index: 0 }).content,
            format!("{}  ", GLYPH_MODE_EDIT)
        );
        let soft = picker_mode_style();
        assert_eq!(soft.fg, Some(Color::Cyan));
        assert!(soft.add_modifier.contains(Modifier::DIM));
        assert_eq!(soft.bg, None);
    }

    #[test]
    fn dashboard_logo_is_six_line_unicode_block_wordmark() {
        use unicode_width::UnicodeWidthStr;

        assert_eq!(DASHBOARD_LOGO.len(), 6);
        assert!(DASHBOARD_LOGO[0].contains("█████"));
        for line in DASHBOARD_LOGO {
            assert!(!line.is_ascii());
            assert!(line.chars().all(|ch| " █╗╔═║╝╚".contains(ch)));
            assert_eq!(UnicodeWidthStr::width(line), 43);
        }
    }

    #[test]
    fn test_log_visual_style_differs_from_selection() {
        install(UiTokens::builtin());
        assert_ne!(log_visual_style().bg, log_selection_style().bg);
    }

    #[test]
    fn test_chip_group_border_style_distinct_from_region_accent() {
        install(UiTokens::builtin());
        assert_ne!(selection_frame(), accent());
        assert_eq!(chip_group_border_style(true).fg, Some(selection_frame()));
    }

    #[test]
    fn test_chip_pill_style_fill() {
        install(UiTokens::builtin());
        let (text, body) = chip_pill_style(ChipField::Tag, "MyTag", false);
        assert_eq!(text, format!(" {} MyTag ", GLYPH_FIELD_TAG));
        assert_eq!(body.bg, Some(accent()));
    }

    #[test]
    fn parse_named_and_hex_colors() {
        assert_eq!(parse_color("cyan").unwrap(), Color::Cyan);
        assert_eq!(parse_color("#0ff").unwrap(), Color::Rgb(0, 255, 255));
        assert_eq!(
            parse_color("#112233").unwrap(),
            Color::Rgb(0x11, 0x22, 0x33)
        );
        assert!(parse_color("nope").is_err());
    }

    #[test]
    fn parse_theme_toml_partial_override() {
        let t = parse_theme_toml("accent = \"red\"\n").unwrap();
        assert_eq!(t.accent, Color::Red);
        assert_eq!(t.success, Color::Green);
    }

    #[test]
    fn canvas_style_reset_on_default_rgb_on_kanagawa() {
        install(map_to_tokens(&Palette::default_ansi()));
        assert_eq!(canvas_bg(), Color::Reset);
        assert_eq!(canvas_style().bg, None);
        assert_eq!(canvas_style().fg, None);
        let p = crate::theme_builtins::palette_by_name("kanagawa").unwrap();
        install(map_to_tokens(&p));
        assert_eq!(canvas_style().bg, Some(p.background));
        assert_eq!(canvas_style().fg, Some(p.foreground));
        install(UiTokens::builtin());
    }

    #[test]
    fn default_map_keeps_cyan_accent_and_reset_canvas() {
        let t = map_to_tokens(&Palette::default_ansi());
        assert_eq!(t.accent, Color::Cyan);
        assert_eq!(t.canvas_bg, Color::Reset);
        assert_eq!(t.log_selection_bg, Color::DarkGray);
        assert_eq!(t.bookmark_row_bg, Color::Rgb(54, 46, 0));
        assert_eq!(t.pkg, Color::LightYellow);
        assert_eq!(t.highlight[0], Color::Yellow);
        assert!(t.logo.iter().all(|c| *c == Color::Cyan));
    }

    #[test]
    fn kanagawa_map_paints_canvas_and_yellow_highlight0() {
        let p = crate::theme_builtins::palette_by_name("kanagawa").unwrap();
        let t = map_to_tokens_for(&p, "kanagawa");
        assert_eq!(t.canvas_bg, Color::Rgb(0x1f, 0x1f, 0x28));
        assert_eq!(t.highlight[0], p.yellow);
        assert_eq!(t.error, p.red);
        assert_ne!(t.log_selection_bg, Color::DarkGray);
        assert_eq!(t.accent, p.blue);
        assert_eq!(t.candidate_match_fg, p.blue);
        assert_eq!(t.logo[0], p.blue);
        assert_eq!(t.logo[5], p.cyan);
        assert_ne!(t.logo[0], t.logo[5]);
    }

    #[test]
    fn named_themes_use_distinct_signature_accents() {
        let cases: &[(&str, fn(&Palette) -> Color)] = &[
            ("onedark", |p| p.blue),
            ("dracula", |p| p.magenta),
            ("everforest", |p| p.green),
            ("tokyo-night", |p| p.blue),
            ("catppuccin-mocha", |p| p.magenta),
            ("gruvbox-dark", |p| p.yellow),
            ("nord", |p| p.cyan),
            ("kanagawa", |p| p.blue),
        ];
        let mut seen = Vec::new();
        for (name, accent_of) in cases {
            let p = crate::theme_builtins::palette_by_name(name).unwrap();
            let t = map_to_tokens_for(&p, name);
            assert_eq!(t.accent, accent_of(&p), "{name} signature accent");
            assert_ne!(
                t.logo[0], t.logo[5],
                "{name} logo is a ramp, not a flat fill"
            );
            seen.push((name, t.accent));
        }
        assert_ne!(seen[1].1, seen[2].1, "dracula magenta ≠ everforest green");
        assert_ne!(seen[5].1, seen[0].1, "gruvbox yellow ≠ onedark blue");
    }

    #[test]
    fn dashboard_logo_and_muted_follow_installed_named_theme() {
        let p = crate::theme_builtins::palette_by_name("dracula").unwrap();
        install(map_to_tokens_for(&p, "dracula"));
        assert_eq!(dashboard_header_style().fg, Some(p.magenta));
        assert_eq!(dashboard_logo_line_style(0).fg, Some(p.cyan));
        assert_eq!(dashboard_logo_line_style(5).fg, Some(p.blue));
        assert!(dashboard_logo_line_style(0)
            .add_modifier
            .contains(Modifier::BOLD));
        assert_eq!(dashboard_muted_style().fg, Some(p.foreground));
        assert!(dashboard_muted_style().add_modifier.contains(Modifier::DIM));
        install(UiTokens::builtin());
        assert_eq!(dashboard_header_style().fg, Some(Color::Cyan));
        assert!(dashboard_logo_line_style(0)
            .fg
            .iter()
            .all(|c| *c == Color::Cyan));
        assert_eq!(dashboard_muted_style().fg, Some(Color::DarkGray));
    }

    #[test]
    fn overlay_accent_overrides_signature_keeps_logo_ramp() {
        let p = crate::theme_builtins::palette_by_name("kanagawa").unwrap();
        let t = apply_overlay_for(p, "kanagawa", "accent = \"#ff00aa\"\n").unwrap();
        assert_eq!(t.accent, Color::Rgb(0xff, 0x00, 0xaa));
        assert_eq!(t.logo[0], p.blue);
        assert_eq!(t.logo[5], p.cyan);
    }

    #[test]
    fn status_pill_on_has_bg_off_is_dim_without_bg() {
        install(UiTokens::builtin());
        let on = status_pill(GLYPH_FOLLOWING, success());
        assert_eq!(on.style.bg, Some(success()));
        assert_eq!(on.style.fg, Some(contrast_fg(success())));
        assert!(on.style.add_modifier.contains(Modifier::BOLD));
        let off = status_icon_dim(GLYPH_FOLLOWING);
        assert_eq!(off.style.bg, None);
        assert!(off.style.add_modifier.contains(Modifier::DIM));
        assert!(!off.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn disconnect_glyph_shares_source_pill_slot() {
        use unicode_width::UnicodeWidthStr;

        install(UiTokens::builtin());
        // nf-fa-plug. nf-fa-chain-broken (f127) ink is ~2 cells and right-biased
        // in non-Mono Nerd Fonts, so the disconnect pill looked wider/shifted.
        assert_eq!(GLYPH_DISCONNECT, "\u{f1e6}");
        assert_ne!(GLYPH_DISCONNECT, "\u{f127}");
        let disconnect = status_pill(GLYPH_DISCONNECT, warning());
        for src in [GLYPH_SOURCE_HDC, GLYPH_SOURCE_ADB, GLYPH_SOURCE_FILE] {
            let connected = status_pill(src, accent());
            assert_eq!(
                UnicodeWidthStr::width(disconnect.content.as_ref()),
                UnicodeWidthStr::width(connected.content.as_ref()),
                "disconnect pill width must match {src:?} source pill"
            );
        }
    }

    #[test]
    fn status_flash_pill_uses_warning_for_failed() {
        install(UiTokens::builtin());
        let failed = status_flash_pill("YANK FAILED");
        assert_eq!(failed.style.bg, Some(warning()));
        assert!(failed.content.contains("YANK FAILED"));
        let ok = status_flash_pill("EXISTS");
        assert_eq!(ok.style.bg, Some(success()));
        assert_ne!(ok.style.bg, failed.style.bg);
    }
}
