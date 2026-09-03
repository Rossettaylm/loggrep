//! Focus-aware keybinding hints for the status bar and Help panel.
//!
//! Status bar uses [`status_hint_entries`] (idle LogList/Strip are curated
//! 1–2 keys; pending and modal surfaces keep the full [`context_entries`]
//! set). The `?` Help panel is two-level: Home (short Active + TOC) and
//! seven zone pages. Copy for contracts and key tables lives here.
//! Rendering is dim keys + normal labels with spacing (no `:` / `|`
//! separators).
//!
//! Key strings come from [`App::keymap`]; labels/details stay in this module.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::keymap::ActionId;
use crate::text_field::TextField;
use crate::theme;

/// Minimum remaining character budget before we bother showing help.
pub const MIN_HELP_WIDTH: usize = 8;

/// Shared `J`/`K` step for LogList cursor movement and Help panel scroll.
pub const FAST_SCROLL_STEP: isize = 7;

/// One keybinding hint (status short label + optional longer Help detail).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintEntry {
    pub key: String,
    pub label: &'static str,
    pub detail: &'static str,
}

impl HintEntry {
    fn new(key: String, label: &'static str, detail: &'static str) -> Self {
        Self { key, label, detail }
    }

    fn short(key: String, label: &'static str) -> Self {
        Self {
            key,
            label,
            detail: label,
        }
    }
}

/// Which situational hint set is active (drives L1/L2 + Help Active).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    Confirm,
    Picker,
    HighlightModal,
    TimePanel,
    HistPanel,
    Detail,
    Leader,
    Bookmark,
    Lock,
    Time,
    ChipField,
    Yank,
    StripD,
    Input,
    ChipStrip,
    ExcludeStrip,
    HighlightStrip,
    LogList,
    LogListLive,
    CommandPalette,
    Compare,
}

impl ContextKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Confirm => "Confirm",
            Self::Picker => "Picker",
            Self::HighlightModal => "Highlight edit",
            Self::TimePanel => "Time window",
            Self::HistPanel => "Histogram",
            Self::Detail => "Detail",
            Self::Leader => "Leader",
            Self::Bookmark => "Bookmark",
            Self::Lock => "Lock",
            Self::Time => "Time",
            Self::ChipField => "Field",
            Self::Yank => "Yank",
            Self::StripD => "Strip delete",
            Self::Input => "Input",
            Self::ChipStrip => "Filter strip",
            Self::ExcludeStrip => "Exclude strip",
            Self::HighlightStrip => "Highlight strip",
            Self::LogList => "Log list",
            Self::LogListLive => "Log list (live)",
            Self::CommandPalette => "Command palette",
            Self::Compare => "Compare",
        }
    }
}

/// Max Active key rows on Help Home (full [`context_entries`] stay elsewhere).
pub const HOME_ACTIVE_LIMIT: usize = 4;

/// Pinned Home/page footer (this panel's chrome; not a zone page).
pub const HOME_CHROME: &str = "/ search    n/N next    h back    Esc close";

/// One of the seven Help zone pages (`1`–`7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpPage {
    Filter,
    Exclude,
    Highlight,
    Log,
    Session,
    Picker,
    Overlays,
}

impl HelpPage {
    pub const ALL: [HelpPage; 7] = [
        Self::Filter,
        Self::Exclude,
        Self::Highlight,
        Self::Log,
        Self::Session,
        Self::Picker,
        Self::Overlays,
    ];

    pub fn index(self) -> u8 {
        match self {
            Self::Filter => 0,
            Self::Exclude => 1,
            Self::Highlight => 2,
            Self::Log => 3,
            Self::Session => 4,
            Self::Picker => 5,
            Self::Overlays => 6,
        }
    }

    pub fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }

    /// Digit `1`–`7` → page. Other chars → `None`.
    pub fn from_digit(c: char) -> Option<Self> {
        let n = c.to_digit(10)?;
        if !(1..=7).contains(&n) {
            return None;
        }
        Self::from_index((n - 1) as u8)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Filter => "Filter",
            Self::Exclude => "Exclude",
            Self::Highlight => "Highlight",
            Self::Log => "Log",
            Self::Session => "Session",
            Self::Picker => "Picker",
            Self::Overlays => "Overlays",
        }
    }
}

/// Help panel view: Home TOC or a zone page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpView {
    Home { toc: u8, toc_off: usize },
    Page { id: HelpPage, scroll: usize },
}

impl Default for HelpView {
    fn default() -> Self {
        Self::Home { toc: 3, toc_off: 0 }
    }
}

/// One ignore-case substring hit in the Help corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpHit {
    /// `None` = Home document; `Some` = that zone page.
    pub page: Option<HelpPage>,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// Optional Help `/` search session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSearch {
    pub query: TextField,
    pub prompt: bool,
    pub hits: Vec<HelpHit>,
    pub current: usize,
}

impl HelpSearch {
    pub fn new() -> Self {
        Self {
            query: TextField::new(),
            prompt: true,
            hits: Vec::new(),
            current: 0,
        }
    }

    pub fn has_hits(&self) -> bool {
        !self.hits.is_empty()
    }
}

/// Styled Help line plus the plain haystack used for search/highlight.
#[derive(Debug, Clone)]
pub struct HelpLine {
    pub text: String,
    pub line: Line<'static>,
}

/// TOC index from the focus that opened Help.
pub fn preselect_toc(focus: Focus) -> u8 {
    match focus {
        Focus::ChipStrip => 0,
        Focus::ExcludeStrip => 1,
        Focus::HighlightStrip => 2,
        Focus::LogList => 3,
        Focus::Input => 0,
    }
}

fn key_of(app: &App, id: ActionId) -> Option<String> {
    let file_mode = app.is_file_mode();
    if !id.meta().allowed(file_mode) {
        return None;
    }
    app.keymap.display(id)
}

fn agg(app: &App, ids: &[ActionId]) -> Option<String> {
    let file_mode = app.is_file_mode();
    let mut parts = Vec::new();
    for &id in ids {
        if !id.meta().allowed(file_mode) {
            continue;
        }
        if let Some(s) = app.keymap.display(id) {
            parts.push(s);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn push_single(
    out: &mut Vec<HintEntry>,
    app: &App,
    id: ActionId,
    label: &'static str,
    detail: &'static str,
) {
    if let Some(key) = key_of(app, id) {
        out.push(HintEntry::new(key, label, detail));
    }
}

fn push_agg(
    out: &mut Vec<HintEntry>,
    app: &App,
    ids: &[ActionId],
    label: &'static str,
    detail: &'static str,
) {
    if let Some(key) = agg(app, ids) {
        out.push(HintEntry::new(key, label, detail));
    }
}

fn push_short(out: &mut Vec<HintEntry>, app: &App, id: ActionId, label: &'static str) {
    push_single(out, app, id, label, label);
}

fn push_literal(out: &mut Vec<HintEntry>, key: &str, label: &'static str, detail: &'static str) {
    out.push(HintEntry::new(key.to_string(), label, detail));
}

/// Resolve the active hint context (modal > pending > focus).
pub fn context_kind(app: &App) -> ContextKind {
    if app.confirm.is_some() {
        return ContextKind::Confirm;
    }
    if app.picker.is_some() {
        return ContextKind::Picker;
    }
    if app.highlight_box.editing {
        return ContextKind::HighlightModal;
    }
    if app.time_panel.is_some() {
        return ContextKind::TimePanel;
    }
    if app.hist_open() {
        return ContextKind::HistPanel;
    }
    if app.compare.is_some() {
        return ContextKind::Compare;
    }
    if app.command_palette.is_some() {
        return ContextKind::CommandPalette;
    }
    if app.detail_open() {
        return ContextKind::Detail;
    }
    if app.pending_leader {
        return ContextKind::Leader;
    }
    if app.pending_m {
        return ContextKind::Bookmark;
    }
    if app.pending_lock {
        return ContextKind::Lock;
    }
    if app.pending_time {
        return ContextKind::Time;
    }
    if app.pending_chip || app.pending_exclude {
        return ContextKind::ChipField;
    }
    if app.pending_yank {
        return ContextKind::Yank;
    }
    if app.pending_d {
        return ContextKind::StripD;
    }
    match app.focus {
        Focus::Input => ContextKind::Input,
        Focus::ChipStrip => ContextKind::ChipStrip,
        Focus::ExcludeStrip => ContextKind::ExcludeStrip,
        Focus::HighlightStrip => ContextKind::HighlightStrip,
        Focus::LogList => {
            if app.export_source.is_live() {
                ContextKind::LogListLive
            } else {
                ContextKind::LogList
            }
        }
    }
}

fn l1_loglist(app: &App, live: bool) -> Vec<HintEntry> {
    let mut out = Vec::new();
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListMoveDown, ActionId::LogListMoveUp],
        "move",
        "move",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListResumeFollow,
        "follow",
        "resume following",
    );
    push_single(&mut out, app, ActionId::GlobalOpenHelp, "help", "open help");
    push_single(
        &mut out,
        app,
        ActionId::GlobalCommandPalette,
        "palette",
        "open command palette",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListLeader,
        "menu",
        "leader then Space for manage",
    );
    push_single(
        &mut out,
        app,
        ActionId::GlobalFilterNew,
        "filter",
        "open filter new",
    );
    push_single(
        &mut out,
        app,
        ActionId::GlobalHighlightNew,
        "highlight",
        "find or create a highlight",
    );
    push_single(
        &mut out,
        app,
        ActionId::GlobalExcludeNew,
        "exclude",
        "open exclude new",
    );
    // mm = bookmark prefix + compare
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::LogListBookmark),
        key_of(app, ActionId::BookmarkManage),
    ) {
        out.push(HintEntry::new(
            format!("{a}{b}"),
            "marks",
            "open compare panel",
        ));
    }
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListNextMatch, ActionId::LogListPrevMatch],
        "next",
        "next",
    );
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListNextSevere, ActionId::LogListPrevSevere],
        "error",
        "error",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListBookmark,
        "mark",
        "bookmark operator",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListLock,
        "focus",
        "lock pid/tid or view focus",
    );
    if !live {
        push_single(&mut out, app, ActionId::LogListTime, "time", "time window");
    }
    push_single(
        &mut out,
        app,
        ActionId::LogListChip,
        "chip",
        "filter from row",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListExcludeChip,
        "exclude",
        "exclude from row",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListYank,
        "yank",
        "yank operator",
    );
    push_agg(
        &mut out,
        app,
        &[ActionId::OpenFile, ActionId::OpenStream],
        "source",
        "open or switch file / stream source",
    );
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListDetailFields, ActionId::LogListDetailPretty],
        "detail",
        "fields / pretty",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListWrapToggle,
        "wrap",
        "toggle single-line collapsed view",
    );
    if live {
        push_single(
            &mut out,
            app,
            ActionId::LogListClearLive,
            "clear",
            "clear buffered logs",
        );
    }
    out
}

fn l1_strip(app: &App) -> Vec<HintEntry> {
    let mut out = Vec::new();
    push_agg(
        &mut out,
        app,
        &[ActionId::StripPrevGroup, ActionId::StripNextGroup],
        "group",
        "group",
    );
    push_single(
        &mut out,
        app,
        ActionId::StripPendingD,
        "del…",
        "dd delete / di disable",
    );
    push_short(&mut out, app, ActionId::StripFocusNext, "focus");
    push_single(
        &mut out,
        app,
        ActionId::StripResumeFollow,
        "follow",
        "resume following",
    );
    push_single(&mut out, app, ActionId::StripOpenHelp, "help", "open help");
    out
}

/// Full L1/L2 for Help Active + catalog. Status bar uses [`status_hint_entries`].
pub fn context_entries(app: &App) -> Vec<HintEntry> {
    match context_kind(app) {
        ContextKind::Confirm => {
            let mut out = Vec::new();
            push_agg(
                &mut out,
                app,
                &[ActionId::ConfirmYes, ActionId::ConfirmYesEnter],
                "confirm",
                "confirm",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::ConfirmNo, ActionId::ConfirmCancel],
                "cancel",
                "cancel",
            );
            out
        }
        ContextKind::Picker => {
            let mut out = Vec::new();
            push_literal(&mut out, "type", "filter", "filter");
            push_agg(
                &mut out,
                app,
                &[ActionId::PickerUp, ActionId::PickerDown],
                "select",
                "select",
            );
            push_single(
                &mut out,
                app,
                ActionId::PickerMulti,
                "multi",
                "toggle multi-select",
            );
            push_single(
                &mut out,
                app,
                ActionId::PickerSubmit,
                "toggle",
                "enable/disable or submit",
            );
            push_single(&mut out, app, ActionId::PickerEdit, "edit", "edit selected");
            push_agg(
                &mut out,
                app,
                &[ActionId::PickerDelete, ActionId::PickerDeleteAlt],
                "delete",
                "delete with confirm",
            );
            push_single(
                &mut out,
                app,
                ActionId::ClearAllRules,
                "clear",
                "clear all rules",
            );
            push_short(&mut out, app, ActionId::PickerClose, "close");
            out
        }
        ContextKind::HighlightModal => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::HighlightModalDraftSpace,
                "draft",
                "space in draft",
            );
            push_agg(
                &mut out,
                app,
                &[
                    ActionId::HighlightModalConfirm,
                    ActionId::HighlightModalConfirmTab,
                ],
                "ok",
                "confirm pattern",
            );
            push_short(&mut out, app, ActionId::HighlightModalCancel, "cancel");
            out
        }
        ContextKind::TimePanel => {
            let mut out = Vec::new();
            push_agg(
                &mut out,
                app,
                &[ActionId::TimePanelNext, ActionId::TimePanelSubmit],
                "next",
                "next field",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::TimePanelDateUp, ActionId::TimePanelDateDown],
                "date",
                "date",
            );
            push_short(&mut out, app, ActionId::TimePanelCancel, "cancel");
            out
        }
        ContextKind::Detail => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::DetailCloseFields, "close");
            push_short(&mut out, app, ActionId::DetailSwap, "swap");
            push_agg(
                &mut out,
                app,
                &[ActionId::DetailChip, ActionId::DetailExclude],
                "chip",
                "filter / exclude field",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::DetailMoveDown, ActionId::DetailMoveUp],
                "row",
                "row",
            );
            push_short(&mut out, app, ActionId::DetailClose, "close");
            out
        }
        ContextKind::Leader => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::LeaderManage,
                "manage",
                "open manage panel",
            );
            push_single(
                &mut out,
                app,
                ActionId::LeaderSummary,
                "stats",
                "open summary panel",
            );
            push_short(&mut out, app, ActionId::LeaderCancel, "cancel");
            out
        }
        ContextKind::Bookmark => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::BookmarkAdd, "add");
            push_short(&mut out, app, ActionId::BookmarkRemove, "delete");
            push_short(&mut out, app, ActionId::BookmarkManage, "compare");
            push_short(&mut out, app, ActionId::BookmarkCancel, "cancel");
            out
        }
        ContextKind::Lock => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::LockPid, "pid");
            push_short(&mut out, app, ActionId::LockTid, "tid");
            push_short(&mut out, app, ActionId::LockViewHighlight, "hl");
            push_short(&mut out, app, ActionId::LockViewSevere, "err");
            push_short(&mut out, app, ActionId::LockClear, "clear");
            push_short(&mut out, app, ActionId::LockCancel, "cancel");
            out
        }
        ContextKind::Time => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::TimeSet, "set");
            push_short(&mut out, app, ActionId::TimeHistogram, "hist");
            push_short(&mut out, app, ActionId::TimeClear, "clear");
            push_short(&mut out, app, ActionId::TimeCancel, "cancel");
            out
        }
        ContextKind::HistPanel => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::HistPanelNext, "next");
            push_short(&mut out, app, ActionId::HistPanelPrev, "prev");
            push_agg(
                &mut out,
                app,
                &[ActionId::HistPanelJumpDown, ActionId::HistPanelJumpUp],
                "fast",
                "move 7 buckets",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::HistPanelJumpTop, ActionId::HistPanelJumpBottom],
                "ends",
                "first or last bucket",
            );
            push_short(&mut out, app, ActionId::HistPanelSubmit, "jump");
            push_short(&mut out, app, ActionId::HistPanelApplyWindow, "window");
            push_agg(
                &mut out,
                app,
                &[ActionId::HistPanelZoomOut, ActionId::HistPanelZoomIn],
                "interval",
                "cycle bucket width 10s/1m/5m",
            );
            push_short(&mut out, app, ActionId::HistPanelCancel, "close");
            out
        }
        ContextKind::ChipField => {
            let mut out = Vec::new();
            for (id, label) in [
                (ActionId::ChipFieldTag, "tag"),
                (ActionId::ChipFieldMsg, "msg"),
                (ActionId::ChipFieldPkg, "pkg"),
                (ActionId::ChipFieldPid, "pid"),
                (ActionId::ChipFieldTid, "tid"),
                (ActionId::ChipFieldLevel, "level"),
                (ActionId::ChipFieldCancel, "cancel"),
            ] {
                push_short(&mut out, app, id, label);
            }
            out
        }
        ContextKind::Yank => {
            let mut out = Vec::new();
            for (id, label) in [
                (ActionId::YankCli, "cli"),
                (ActionId::YankTag, "tag"),
                (ActionId::YankMsg, "msg"),
                (ActionId::YankPkg, "pkg"),
                (ActionId::YankPid, "pid"),
                (ActionId::YankTid, "tid"),
                (ActionId::YankLevel, "level"),
                (ActionId::YankRaw, "raw"),
                (ActionId::YankLine, "line"),
                (ActionId::YankTime, "time"),
                (ActionId::YankCancel, "cancel"),
            ] {
                push_short(&mut out, app, id, label);
            }
            out
        }
        ContextKind::StripD => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::StripDDelete, "delete");
            push_short(&mut out, app, ActionId::StripDDisable, "disable");
            push_short(&mut out, app, ActionId::StripDCancel, "cancel");
            out
        }
        ContextKind::Input => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::InputDraftSpace,
                "draft",
                "space in draft",
            );
            push_single(
                &mut out,
                app,
                ActionId::InputCommit,
                "commit",
                "pill then submit group",
            );
            push_single(
                &mut out,
                app,
                ActionId::InputToggleExclude,
                "exclude",
                "toggle exclude draft",
            );
            push_short(&mut out, app, ActionId::InputCancel, "cancel");
            out
        }
        ContextKind::ChipStrip | ContextKind::ExcludeStrip | ContextKind::HighlightStrip => {
            l1_strip(app)
        }
        ContextKind::LogList => l1_loglist(app, false),
        ContextKind::LogListLive => l1_loglist(app, true),
        ContextKind::CommandPalette => {
            let mut out = Vec::new();
            push_literal(&mut out, "type", "filter", "type to filter commands");
            push_agg(
                &mut out,
                app,
                &[ActionId::PaletteUp, ActionId::PaletteDown],
                "select",
                "select",
            );
            push_single(
                &mut out,
                app,
                ActionId::PaletteSubmit,
                "run",
                "run selected command",
            );
            push_short(&mut out, app, ActionId::PaletteClose, "close");
            out
        }
        ContextKind::Compare => {
            let mut out = Vec::new();
            push_literal(&mut out, "j/k", "pin", "move by pin");
            push_literal(&mut out, "g/G", "first", "first / last pin");
            push_literal(&mut out, "yy", "yank", "yank selected snapshot raw");
            push_literal(&mut out, "dd", "delete", "delete selected pin");
            push_literal(&mut out, "Enter", "jump", "jump to origin in log");
            push_literal(&mut out, "Esc", "close", "close compare panel");
            out
        }
    }
}

/// Status-bar hint subset: idle LogList/Strip are curated 1–2 keys;
/// pending/modal surfaces keep the full [`context_entries`] set.
pub fn status_hint_entries(app: &App) -> Vec<HintEntry> {
    match context_kind(app) {
        ContextKind::LogList | ContextKind::LogListLive => {
            let mut out = Vec::new();
            push_single(&mut out, app, ActionId::GlobalOpenHelp, "help", "open help");
            push_single(
                &mut out,
                app,
                ActionId::GlobalFilterNew,
                "filter",
                "open filter new",
            );
            out
        }
        ContextKind::ChipStrip | ContextKind::ExcludeStrip | ContextKind::HighlightStrip => {
            let mut out = Vec::new();
            push_single(&mut out, app, ActionId::StripOpenHelp, "help", "open help");
            push_single(
                &mut out,
                app,
                ActionId::StripPendingD,
                "del…",
                "dd delete / di disable",
            );
            out
        }
        _ => context_entries(app),
    }
}

/// Design-contract copy for a zone page (at most 5 lines).
pub fn page_blurb(page: HelpPage) -> &'static [&'static str] {
    match page {
        HelpPage::Filter => &[
            "Filter chips live in groups.",
            "Inside a group every chip is AND; across enabled groups the result is OR.",
            "If every group is disabled, that is the same as an empty list: every row stays visible.",
            "New filters go through the Picker, not the old Input strip.",
            "Startup CLI flags become group 0 and can be deleted or disabled like any other group.",
        ],
        HelpPage::Exclude => &[
            "Exclude groups apply as global AND NOT after Filter (then lock and the time window).",
            "They are not an inverted Filter page: a row must pass Filter and then match no enabled Exclude.",
            "Empty Exclude strip is folded.",
            "`C` plus a field letter pushes an exclude from the current row.",
        ],
        HelpPage::Highlight => &[
            "Highlight groups paint matching text; they do not hide rows.",
            "Enabled patterns are OR and walk the 8-slot color ramp in order.",
            "`/` on LogList finds or creates a highlight and jumps to the first hit; existing hits stay listed, then a create row for a non-exact query. `/` inside this Help panel is search and does not create a highlight.",
            "Command palette Add Highlight always opens New. Unified Enter still toggles enable without jumping.",
        ],
        HelpPage::Log => &[
            "The log list is the action origin: most cancels return here.",
            "Leaving the last visible row pauses following; landing on the last row resumes it; Esc still resumes explicitly.",
            "Yank, wrap, visual, and chip-from-row start on the current line.",
            "File mode can browse the whole file; live mode is a dropping ring.",
        ],
        HelpPage::Session => &[
            "Lock PID and lock TID are mutually exclusive and AND after chips.",
            "The global time window is file-only and orthogonal to Filter groups.",
            "`th` opens a time histogram over the current visible rows; Enter jumps, s sets the window, J/K/g/G move, Tab cycles 10s/1m/5m.",
            "Bookmarks are session-only snapshot pins with a compare panel (`mm`).",
            "Follow and device/file state live in the status bar, not in a chip group.",
        ],
        HelpPage::Picker => &[
            "Space is Leader; Space Space opens unified Manage.",
            "Bare `;` and backtick force New for Filter / Exclude. `/` finds or creates a Highlight.",
            "Unified Manage stays in Manage when nothing matches. Highlight finder appends a create row when the query is not an exact existing pattern, and auto-opens New on zero hits.",
            "`C-t` in unified Manage flips every target on or off: Tab-checked rows when any are checked, else the query-visible rows.",
            "The compare tray (`mm`) is not this unified picker.",
        ],
        HelpPage::Overlays => &[
            "Fields (`p`) and Pretty (`P`) are a top modal on the current row; Esc closes the overlay only and does not resume following.",
            "Pretty pretty-prints JSON in msg (then raw).",
            "The command palette (`C-p`) is not a Picker: an empty query shows no list.",
            "File-mode `th` opens a time histogram over visible rows (Enter jumps, s sets the window, Tab cycles 10s/1m/5m).",
            "This Help panel's own keys are on the Home footer, not in this list.",
        ],
    }
}

fn push_strip_group_ops(out: &mut Vec<HintEntry>, app: &App) {
    push_agg(
        out,
        app,
        &[ActionId::StripPrevGroup, ActionId::StripNextGroup],
        "group",
        "prev / next group on focused strip",
    );
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::StripPendingD),
        key_of(app, ActionId::StripDDelete),
    ) {
        out.push(HintEntry::new(
            format!("{a}{b}"),
            "delete",
            "delete selected strip group",
        ));
    }
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::StripPendingD),
        key_of(app, ActionId::StripDDisable),
    ) {
        out.push(HintEntry::new(
            format!("{a}{b}"),
            "disable",
            "toggle disable selected strip group",
        ));
    }
}

fn page_entries(app: &App, page: HelpPage) -> Vec<HintEntry> {
    let live = app.export_source.is_live();
    match page {
        HelpPage::Filter => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::GlobalFilterNew,
                "filter new",
                "open filter picker in new mode",
            );
            push_strip_group_ops(&mut out, app);
            push_single(
                &mut out,
                app,
                ActionId::LogListChip,
                "chip",
                "filter/highlight from row (msg → tokens → Filter|Highlight)",
            );
            out
        }
        HelpPage::Exclude => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::GlobalExcludeNew,
                "exclude new",
                "open exclude picker in new mode",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListExcludeChip,
                "exclude",
                "exclude chip from current row field",
            );
            push_strip_group_ops(&mut out, app);
            out
        }
        HelpPage::Highlight => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::GlobalHighlightNew,
                "find highlight",
                "find or create a highlight and jump to the first hit",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListNextMatch, ActionId::LogListPrevMatch],
                "next hit",
                "next / previous highlight match",
            );
            push_strip_group_ops(&mut out, app);
            out
        }
        HelpPage::Log => {
            let mut out = Vec::new();
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListMoveDown, ActionId::LogListMoveUp],
                "move",
                "move cursor one line",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListJumpDown, ActionId::LogListJumpUp],
                "jump",
                "move 7 lines",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListJumpTop, ActionId::LogListJumpBottom],
                "top/bottom",
                "jump top or bottom (G resumes follow)",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListResumeFollow,
                "follow",
                "resume following and pin to bottom",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListNextSevere, ActionId::LogListPrevSevere],
                "error",
                "next / previous severe line",
            );
            push_literal(
                &mut out,
                "1-5",
                "focus",
                "focus filter / exclude / highlight / log / input",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListWrapToggle,
                "wrap",
                "toggle multi-line wrap / single-line collapsed view",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListVisualLine,
                "visual",
                "visual line mode",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListYankMsgLine,
                "yank msg",
                "yank message of current line",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListChip,
                "chip",
                "filter/highlight from row (msg → tokens → Filter|Highlight)",
            );
            if let (Some(y), Some(c)) = (
                key_of(app, ActionId::LogListYank),
                key_of(app, ActionId::YankCli),
            ) {
                out.push(HintEntry::new(
                    format!("{y} {c}"),
                    "export",
                    "yank filters as alnav grep CLI (literal approx)",
                ));
            }
            if let Some(y) = key_of(app, ActionId::LogListYank) {
                out.push(HintEntry::new(
                    format!("{y} …"),
                    "yank field",
                    "yank tag/msg(token picker)/pkg/pid/tid/level/raw/line/time",
                ));
            }
            out
        }
        HelpPage::Session => {
            let mut session = Vec::new();
            push_single(
                &mut session,
                app,
                ActionId::LeaderPresetSave,
                "save preset",
                "save Filter/Exclude/Highlight preset",
            );
            push_single(
                &mut session,
                app,
                ActionId::LeaderPresetOpen,
                "open preset",
                "search and apply named preset",
            );
            push_agg(
                &mut session,
                app,
                &[ActionId::OpenFile, ActionId::OpenStream],
                "source",
                "open or switch file / stream source",
            );
            if let (Some(p), Some(fp), Some(ft), Some(fu)) = (
                key_of(app, ActionId::LogListLock),
                key_of(app, ActionId::LockPid),
                key_of(app, ActionId::LockTid),
                key_of(app, ActionId::LockClear),
            ) {
                session.push(HintEntry::new(
                    format!("{p} {fp}/{ft}/{fu}"),
                    "lock",
                    "lock pid / tid / clear",
                ));
            }
            if let (Some(p), Some(h), Some(e)) = (
                key_of(app, ActionId::LogListLock),
                key_of(app, ActionId::LockViewHighlight),
                key_of(app, ActionId::LockViewSevere),
            ) {
                session.push(HintEntry::new(
                    format!("{p} {h}/{e}"),
                    "view",
                    "highlight-only / severe-only (independent toggles; both = AND)",
                ));
            }
            if !live {
                if let (Some(t), Some(tt), Some(tu)) = (
                    key_of(app, ActionId::LogListTime),
                    key_of(app, ActionId::TimeSet),
                    key_of(app, ActionId::TimeClear),
                ) {
                    session.push(HintEntry::new(
                        format!("{t} {tt}/{tu}"),
                        "time",
                        "set / clear global time window (file only)",
                    ));
                }
                if let (Some(t), Some(th)) = (
                    key_of(app, ActionId::LogListTime),
                    key_of(app, ActionId::TimeHistogram),
                ) {
                    session.push(HintEntry::new(
                        format!("{t} {th}"),
                        "hist",
                        "time histogram: jump or set window (file only)",
                    ));
                }
            }
            if let (Some(m), Some(a), Some(d)) = (
                key_of(app, ActionId::LogListBookmark),
                key_of(app, ActionId::BookmarkAdd),
                key_of(app, ActionId::BookmarkRemove),
            ) {
                session.push(HintEntry::new(
                    format!("{m}{a}/{m}{d}"),
                    "bookmark",
                    "add / remove bookmark on current row",
                ));
            }
            if let (Some(m), Some(b)) = (
                key_of(app, ActionId::LogListBookmark),
                key_of(app, ActionId::BookmarkManage),
            ) {
                session.push(HintEntry::new(
                    format!("{m}{b}"),
                    "bookmarks",
                    "open bookmark compare panel",
                ));
            }
            if live {
                push_single(
                    &mut session,
                    app,
                    ActionId::LogListClearLive,
                    "clear",
                    "clear buffered live logs",
                );
            }
            session
        }
        HelpPage::Picker => {
            let mut leader = Vec::new();
            if let (Some(a), Some(b)) = (
                key_of(app, ActionId::LogListLeader),
                key_of(app, ActionId::LeaderManage),
            ) {
                leader.push(HintEntry::new(
                    format!("{a} {b}"),
                    "manage",
                    "unified manage picker",
                ));
            }
            push_single(
                &mut leader,
                app,
                ActionId::GlobalFilterNew,
                "filter new",
                "open filter picker in new mode",
            );
            push_single(
                &mut leader,
                app,
                ActionId::GlobalHighlightNew,
                "find highlight",
                "find or create a highlight and jump to the first hit",
            );
            push_single(
                &mut leader,
                app,
                ActionId::GlobalExcludeNew,
                "exclude new",
                "open exclude picker in new mode",
            );
            push_literal(
                &mut leader,
                "type",
                "filter",
                "type to fuzzy-filter; Enter toggle; ^X edit; Del delete",
            );
            push_single(
                &mut leader,
                app,
                ActionId::PickerEdit,
                "edit",
                "edit selected",
            );
            push_agg(
                &mut leader,
                app,
                &[ActionId::PickerDelete, ActionId::PickerDeleteAlt],
                "delete",
                "delete with confirm",
            );
            leader
        }
        HelpPage::Overlays => {
            let mut overlays = Vec::new();
            push_agg(
                &mut overlays,
                app,
                &[ActionId::LogListDetailFields, ActionId::LogListDetailPretty],
                "detail",
                "toggle fields / pretty overlay",
            );
            push_single(
                &mut overlays,
                app,
                ActionId::GlobalCommandPalette,
                "palette",
                "open command palette",
            );
            if let (Some(a), Some(b)) = (
                key_of(app, ActionId::LogListLeader),
                key_of(app, ActionId::LeaderSummary),
            ) {
                overlays.push(HintEntry::new(
                    format!("{a} {b}"),
                    "stats",
                    "open summary panel (level / tags / errors)",
                ));
            }
            if !live {
                push_agg(
                    &mut overlays,
                    app,
                    &[ActionId::TimePanelNext, ActionId::TimePanelSubmit],
                    "next",
                    "time panel next field",
                );
                push_agg(
                    &mut overlays,
                    app,
                    &[ActionId::TimePanelDateUp, ActionId::TimePanelDateDown],
                    "date",
                    "time panel date",
                );
                push_short(&mut overlays, app, ActionId::TimePanelCancel, "cancel");
            }
            overlays
        }
    }
}

fn styled_help_line(line: Line<'static>) -> HelpLine {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    HelpLine { text, line }
}

fn plain_help_line(text: impl Into<String>, style: Style) -> HelpLine {
    let text = text.into();
    HelpLine {
        line: Line::from(Span::styled(text.clone(), style)),
        text,
    }
}

/// Home Active block: title + at most [`HOME_ACTIVE_LIMIT`] context entries.
pub fn home_active_lines(app: &App) -> Vec<HelpLine> {
    let kind = context_kind(app);
    let title = Line::from(vec![
        Span::styled(
            "Active  ",
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            kind.title().to_string(),
            Style::default().fg(theme::accent()),
        ),
    ]);
    let mut lines = vec![styled_help_line(title)];
    for entry in context_entries(app).into_iter().take(HOME_ACTIVE_LIMIT) {
        lines.push(styled_help_line(detail_line(&entry)));
    }
    lines
}

/// Numbered TOC rows (`1`–`7`). `selected` is highlighted when `Some`.
pub fn home_toc_lines(selected: Option<u8>) -> Vec<HelpLine> {
    HelpPage::ALL
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let text = format!("{}  {}", i + 1, page.title());
            let selected = selected == Some(i as u8);
            let style = if selected {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            HelpLine {
                line: Line::from(Span::styled(text.clone(), style)),
                text,
            }
        })
        .collect()
}

pub fn chrome_help_line() -> HelpLine {
    let mut spans = Vec::new();
    for (i, (key, label)) in [
        ("/", "search"),
        ("n/N", "next"),
        ("h", "back"),
        ("Esc", "close"),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            spans.push(Span::raw("    "));
        }
        spans.push(Span::styled(key.to_string(), key_style()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(label.to_string(), label_style()));
    }
    HelpLine {
        text: HOME_CHROME.to_string(),
        line: Line::from(spans),
    }
}

/// Home document for search: Active + TOC (unselected) + chrome.
pub fn home_doc_lines(app: &App) -> Vec<HelpLine> {
    let mut lines = home_active_lines(app);
    lines.extend(home_toc_lines(None));
    lines.push(chrome_help_line());
    lines
}

/// Zone page document: title, blurb, key table.
pub fn page_doc_lines(app: &App, page: HelpPage) -> Vec<HelpLine> {
    let mut lines = Vec::new();
    lines.push(plain_help_line(
        page.title().to_string(),
        Style::default()
            .fg(theme::accent())
            .add_modifier(Modifier::BOLD),
    ));
    for blurb in page_blurb(page) {
        lines.push(plain_help_line((*blurb).to_string(), label_style()));
    }
    lines.push(HelpLine {
        text: String::new(),
        line: Line::from(""),
    });
    for entry in page_entries(app, page) {
        lines.push(styled_help_line(detail_line(&entry)));
    }
    lines
}

/// Max page `scroll` so the last line sits at the bottom of the viewport
/// instead of the top (which would leave a blank remainder).
pub fn page_max_scroll(line_count: usize, viewport_rows: usize) -> usize {
    line_count.saturating_sub(viewport_rows.max(1))
}

/// Lines for the current Help view (unwindowed; tests + modal height).
pub fn help_body_lines(app: &App) -> Vec<Line<'static>> {
    match app.help_view {
        HelpView::Home { toc, .. } => {
            let mut lines: Vec<Line<'static>> =
                home_active_lines(app).into_iter().map(|l| l.line).collect();
            lines.extend(home_toc_lines(Some(toc)).into_iter().map(|l| l.line));
            lines.push(chrome_help_line().line);
            lines
        }
        HelpView::Page { id, .. } => {
            let mut lines: Vec<Line<'static>> = page_doc_lines(app, id)
                .into_iter()
                .map(|l| l.line)
                .collect();
            lines.push(chrome_help_line().line);
            lines
        }
    }
}

pub fn home_active_len(app: &App) -> usize {
    home_active_lines(app).len()
}

fn substring_spans(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    h.match_indices(&n).map(|(i, m)| (i, i + m.len())).collect()
}

/// Ignore-case substring hits across Home + all seven pages.
pub fn search_help_hits(app: &App, query: &str) -> Vec<HelpHit> {
    let needle = query.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (line_idx, row) in home_doc_lines(app).iter().enumerate() {
        for (start, end) in substring_spans(&row.text, needle) {
            hits.push(HelpHit {
                page: None,
                line: line_idx,
                start,
                end,
            });
        }
    }
    for page in HelpPage::ALL {
        for (line_idx, row) in page_doc_lines(app, page).iter().enumerate() {
            for (start, end) in substring_spans(&row.text, needle) {
                hits.push(HelpHit {
                    page: Some(page),
                    line: line_idx,
                    start,
                    end,
                });
            }
        }
    }
    hits
}

pub fn hits_on_line(
    search: Option<&HelpSearch>,
    page: Option<HelpPage>,
    line: usize,
) -> Vec<(usize, usize, bool)> {
    let Some(search) = search else {
        return Vec::new();
    };
    if search.hits.is_empty() {
        return Vec::new();
    }
    search
        .hits
        .iter()
        .enumerate()
        .filter(|(_, hit)| hit.page == page && hit.line == line)
        .map(|(i, hit)| (hit.start, hit.end, i == search.current))
        .collect()
}

/// Overlay substring hits onto an already-styled line.
pub fn overlay_search_hits(line: Line<'static>, hits: &[(usize, usize, bool)]) -> Line<'static> {
    if hits.is_empty() {
        return line;
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut byte = 0usize;
    for span in line.spans {
        let content = span.content.into_owned();
        let style = span.style;
        let end = byte + content.len();
        let mut cursor = 0usize;
        let mut marks: Vec<(usize, usize, bool)> = hits
            .iter()
            .filter_map(|&(s, e, cur)| {
                let a = s.max(byte);
                let b = e.min(end);
                if a < b {
                    Some((a - byte, b - byte, cur))
                } else {
                    None
                }
            })
            .collect();
        marks.sort_by_key(|m| (m.0, !m.2, m.1));
        for (s, e, current) in marks {
            let s = s.max(cursor);
            if s > cursor {
                out.push(Span::styled(content[cursor..s].to_string(), style));
            }
            if e > s {
                let hit_style = if current {
                    theme::help_search_current_style()
                } else {
                    theme::help_search_hit_style()
                };
                out.push(Span::styled(content[s..e].to_string(), hit_style));
                cursor = e;
            }
        }
        if cursor < content.len() {
            out.push(Span::styled(content[cursor..].to_string(), style));
        }
        byte = end;
    }
    Line::from(out)
}

pub fn decode_home_hit_line(app: &App, line: usize) -> HomeHitKind {
    let active = home_active_len(app);
    if line < active {
        HomeHitKind::Active
    } else if line < active + HelpPage::ALL.len() {
        HomeHitKind::Toc((line - active) as u8)
    } else {
        HomeHitKind::Chrome
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeHitKind {
    Active,
    Toc(u8),
    Chrome,
}

/// Whether Help may open for the current app state.
pub fn help_available(app: &App) -> bool {
    if app.confirm.is_some()
        || app.picker.is_some()
        || app.time_panel.is_some()
        || app.hist_open()
        || app.detail_open()
        || app.highlight_box.editing
        || app.command_palette.is_some()
        || app.compare.is_some()
    {
        return false;
    }
    if app.pending_leader
        || app.pending_m
        || app.pending_lock
        || app.pending_time
        || app.pending_chip
        || app.pending_exclude
        || app.pending_yank
        || app.pending_d
    {
        return false;
    }
    matches!(
        app.focus,
        Focus::LogList | Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
    )
}

fn key_style() -> Style {
    theme::context_help_style()
}

fn label_style() -> Style {
    Style::default()
}

fn entry_width(entry: &HintEntry) -> usize {
    entry.key.chars().count() + 1 + entry.label.chars().count()
}

/// Fit situational hints into `max_chars` as styled spans (dim key + label).
pub fn context_hint_spans(app: &App, max_chars: usize) -> Option<Vec<Span<'static>>> {
    if max_chars < MIN_HELP_WIDTH {
        return None;
    }
    let entries = status_hint_entries(app);
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let gap = if i == 0 { 0 } else { 2 };
        let need = gap + entry_width(entry);
        if used + need <= max_chars {
            if gap > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(entry.key.clone(), key_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(entry.label.to_string(), label_style()));
            used += need;
            continue;
        }
        let key_w = entry.key.chars().count();
        let remain = max_chars.saturating_sub(used + gap + key_w + 1);
        if remain >= 1 && used + gap + key_w + 1 < max_chars {
            if gap > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(entry.key.clone(), key_style()));
            spans.push(Span::raw(" "));
            let trunc: String = entry.label.chars().take(remain).collect();
            spans.push(Span::styled(trunc, label_style()));
        }
        break;
    }
    if spans.is_empty() {
        None
    } else {
        Some(spans)
    }
}

fn detail_line(entry: &HintEntry) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<12}", entry.key), key_style()),
        Span::styled(entry.detail.to_string(), label_style()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Focus;

    fn app_with_focus(focus: Focus) -> App {
        let mut app = App::new(100);
        app.focus = focus;
        app
    }

    #[test]
    fn context_kind_by_focus() {
        assert_eq!(
            context_kind(&app_with_focus(Focus::LogList)),
            ContextKind::LogList
        );
        assert_eq!(
            context_kind(&app_with_focus(Focus::ChipStrip)),
            ContextKind::ChipStrip
        );
        assert_eq!(
            context_kind(&app_with_focus(Focus::ExcludeStrip)),
            ContextKind::ExcludeStrip
        );
        assert_eq!(
            context_kind(&app_with_focus(Focus::HighlightStrip)),
            ContextKind::HighlightStrip
        );
        assert_eq!(
            context_kind(&app_with_focus(Focus::Input)),
            ContextKind::Input
        );
    }

    #[test]
    fn loglist_entries_include_help() {
        let entries = context_entries(&app_with_focus(Focus::LogList));
        assert!(
            entries.iter().any(|e| e.key == "?" && e.label == "help"),
            "LogList L1 must expose ? help"
        );
    }

    #[test]
    fn loglist_entries_include_wrap_toggle() {
        let entries = context_entries(&app_with_focus(Focus::LogList));
        assert!(
            entries.iter().any(|e| e.key == "w" && e.label == "wrap"),
            "LogList L1 must expose w wrap toggle"
        );
    }

    #[test]
    fn context_loglist_live_appends_clear_hint() {
        let mut app = app_with_focus(Focus::LogList);
        assert_eq!(context_kind(&app), ContextKind::LogList);
        app.export_source = crate::export::ExportSource::Hdc { device: None };
        assert_eq!(context_kind(&app), ContextKind::LogListLive);
        let entries = context_entries(&app);
        assert!(
            entries.iter().any(|e| e.key == "C-l"),
            "live LogList hint must expose Ctrl-L clear"
        );
        assert!(
            !entries.iter().any(|e| e.key == "t"),
            "live LogList must not expose interactive time"
        );

        app.export_source = crate::export::ExportSource::Adb { device: None };
        assert_eq!(context_kind(&app), ContextKind::LogListLive);
    }

    #[test]
    fn context_search_modal_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.highlight_box.editing = true;
        assert_eq!(context_kind(&app), ContextKind::HighlightModal);
    }

    #[test]
    fn context_msg_chip_picker_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.open_picker(crate::picker::PickerKind::MsgChip {
            purpose: crate::picker::MsgChipPurpose::Chip { exclude: false },
        });
        assert_eq!(context_kind(&app), ContextKind::Picker);
    }

    #[test]
    fn context_confirm_overrides_picker() {
        use crate::picker::{UnifiedId, UnifiedKind};

        let mut app = app_with_focus(Focus::LogList);
        app.open_unified_picker();
        app.request_delete_many(vec![
            UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 0,
            },
            UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 1,
            },
            UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 2,
            },
        ]);
        assert_eq!(context_kind(&app), ContextKind::Confirm);
    }

    #[test]
    fn context_pending_leader_is_l2() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_leader = true;
        assert_eq!(context_kind(&app), ContextKind::Leader);
    }

    #[test]
    fn context_detail_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.detail = crate::app::DetailView::Fields;
        assert_eq!(context_kind(&app), ContextKind::Detail);
    }

    #[test]
    fn context_pending_ops_are_l2() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_m = true;
        assert_eq!(context_kind(&app), ContextKind::Bookmark);
        app.pending_m = false;
        app.pending_lock = true;
        assert_eq!(context_kind(&app), ContextKind::Lock);
        app.pending_lock = false;
        app.pending_time = true;
        assert_eq!(context_kind(&app), ContextKind::Time);
        app.pending_time = false;
        app.pending_chip = true;
        assert_eq!(context_kind(&app), ContextKind::ChipField);
        app.pending_chip = false;
        app.pending_exclude = true;
        assert_eq!(context_kind(&app), ContextKind::ChipField);
        app.pending_exclude = false;
        app.pending_yank = true;
        assert_eq!(context_kind(&app), ContextKind::Yank);
        app.pending_yank = false;
        app.focus = Focus::ChipStrip;
        app.pending_d = true;
        assert_eq!(context_kind(&app), ContextKind::StripD);
    }

    #[test]
    fn context_modal_beats_pending() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_m = true;
        app.highlight_box.editing = true;
        assert_eq!(context_kind(&app), ContextKind::HighlightModal);
    }

    #[test]
    fn hint_spans_hide_when_too_narrow() {
        let app = app_with_focus(Focus::LogList);
        assert!(context_hint_spans(&app, MIN_HELP_WIDTH - 1).is_none());
    }

    #[test]
    fn hint_spans_fit_without_colon() {
        let app = app_with_focus(Focus::LogList);
        let spans = context_hint_spans(&app, 200).expect("wide enough");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.contains(':'), "no colon separators: {text:?}");
        assert!(text.contains("help"), "expected help hint: {text:?}");
        assert!(
            text.contains("filter"),
            "idle LogList must show filter: {text:?}"
        );
        assert!(
            !text.contains("j/k move"),
            "idle LogList must not dump full L1: {text:?}"
        );
    }

    #[test]
    fn status_idle_loglist_is_help_and_filter() {
        let app = app_with_focus(Focus::LogList);
        let entries = status_hint_entries(&app);
        let labels: Vec<&str> = entries.iter().map(|e| e.label).collect();
        assert_eq!(labels, ["help", "filter"], "{entries:?}");
        let live = {
            let mut app = app_with_focus(Focus::LogList);
            app.export_source = crate::export::ExportSource::Hdc { device: None };
            status_hint_entries(&app)
        };
        let live_labels: Vec<&str> = live.iter().map(|e| e.label).collect();
        assert_eq!(live_labels, ["help", "filter"], "{live:?}");
    }

    #[test]
    fn status_idle_strip_is_help_and_del() {
        for focus in [Focus::ChipStrip, Focus::ExcludeStrip, Focus::HighlightStrip] {
            let entries = status_hint_entries(&app_with_focus(focus));
            let labels: Vec<&str> = entries.iter().map(|e| e.label).collect();
            assert_eq!(labels, ["help", "del…"], "{focus:?} {entries:?}");
        }
    }

    #[test]
    fn status_pending_chip_lists_fields() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_chip = true;
        let spans = context_hint_spans(&app, 200).expect("wide enough");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("tag"), "{text:?}");
        assert!(text.contains("msg"), "{text:?}");
        assert!(
            !text.contains("c…"),
            "pending prefix must not leak: {text:?}"
        );
    }

    #[test]
    fn status_pending_and_modal_keep_full_context_entries() {
        let labels = |app: &crate::app::App| -> Vec<&str> {
            status_hint_entries(app).iter().map(|e| e.label).collect()
        };
        let full = |app: &crate::app::App| -> Vec<&str> {
            context_entries(app).iter().map(|e| e.label).collect()
        };

        let mut app = app_with_focus(Focus::LogList);
        app.pending_chip = true;
        assert_eq!(labels(&app), full(&app), "pending must not use idle 1–2");

        app.pending_chip = false;
        app.detail = crate::app::DetailView::Fields;
        assert_eq!(labels(&app), full(&app), "Detail must expand full set");
        assert!(
            labels(&app).len() > 2,
            "Detail must not keep idle help+filter: {:?}",
            labels(&app)
        );

        app.detail = crate::app::DetailView::Closed;
        app.open_picker(crate::picker::PickerKind::Filter);
        assert_eq!(labels(&app), full(&app), "Picker must expand full set");
    }

    fn joined_lines(lines: &[Line<'static>]) -> String {
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

    #[test]
    fn help_body_still_lists_move_cursor() {
        let app = app_with_focus(Focus::LogList);
        let home = joined_lines(&help_body_lines(&app));
        assert!(home.contains("move"), "{home}");
        let active = context_entries(&app);
        assert!(
            active.iter().any(|e| e.label == "move"),
            "Help Active must keep full LogList L1: {active:?}"
        );
        let log = joined_lines(
            &page_doc_lines(&app, HelpPage::Log)
                .into_iter()
                .map(|l| l.line)
                .collect::<Vec<_>>(),
        );
        assert!(log.contains("move cursor") || log.contains("move"), "{log}");
    }

    #[test]
    fn help_available_on_strips_not_when_pending() {
        let mut app = app_with_focus(Focus::ChipStrip);
        assert!(help_available(&app));
        app.pending_d = true;
        assert!(!help_available(&app));
        app.pending_d = false;
        app.open_picker(crate::picker::PickerKind::Filter);
        assert!(!help_available(&app));
    }

    #[test]
    fn help_home_has_active_and_toc() {
        let app = app_with_focus(Focus::LogList);
        let text = joined_lines(&help_body_lines(&app));
        assert!(text.contains("Active"), "{text}");
        assert!(text.contains("Filter"), "{text}");
        assert!(text.contains("1"), "{text}");
        assert!(!text.contains("All commands"), "{text}");
    }

    #[test]
    fn adb_help_hides_time_and_shows_clear() {
        let mut app = app_with_focus(Focus::LogList);
        app.export_source = crate::export::ExportSource::Adb { device: None };
        let text = joined_lines(
            &page_doc_lines(&app, HelpPage::Session)
                .into_iter()
                .map(|l| l.line)
                .collect::<Vec<_>>(),
        );

        assert!(!text.contains("set / clear global time window"), "{text}");
        assert!(text.contains("clear buffered live logs"), "{text}");
    }

    #[test]
    fn catalog_jk_details_match_fast_scroll_step() {
        let app = app_with_focus(Focus::LogList);
        let step = FAST_SCROLL_STEP.to_string();
        let log_entries = page_entries(&app, HelpPage::Log);
        let nav_jk = log_entries
            .iter()
            .find(|e| e.key.contains('/') && e.label == "jump")
            .expect("log jump");
        assert!(
            nav_jk.detail.contains(&step),
            "log jump detail {:?} must mention FAST_SCROLL_STEP={step}",
            nav_jk.detail
        );
    }

    #[test]
    fn page_blurbs_are_at_most_five_lines() {
        for page in HelpPage::ALL {
            let n = page_blurb(page).len();
            assert!(n <= 5, "{:?} blurb has {n} lines", page);
            assert!(n >= 1, "{:?} missing blurb", page);
        }
    }

    #[test]
    fn filter_page_lists_filter_new_overlays_omits_help_search() {
        let app = app_with_focus(Focus::LogList);
        let filter = page_entries(&app, HelpPage::Filter);
        assert!(filter.iter().any(|e| e.label == "filter new"), "{filter:?}");
        let highlight = page_entries(&app, HelpPage::Highlight);
        assert!(
            highlight.iter().any(|e| e.label == "find highlight"),
            "{highlight:?}"
        );
        let picker = joined_lines(
            &page_doc_lines(&app, HelpPage::Picker)
                .into_iter()
                .map(|l| l.line)
                .collect::<Vec<_>>(),
        );
        assert!(
            !picker.contains("force New for Filter / Highlight"),
            "Picker blurb must not call / force New: {picker}"
        );
        assert!(
            picker.contains("finds or creates") || picker.contains("find-or-create"),
            "{picker}"
        );
        let overlays = joined_lines(
            &page_doc_lines(&app, HelpPage::Overlays)
                .into_iter()
                .map(|l| l.line)
                .collect::<Vec<_>>(),
        );
        assert!(
            !overlays.contains("search help") && !overlays.to_lowercase().contains("/ search"),
            "Overlays must not list Help chrome /: {overlays}"
        );
        assert!(
            overlays.contains("command palette") || overlays.contains("C-p"),
            "{overlays}"
        );
    }

    #[test]
    fn log_page_owns_wrap_visual_yank_overlays_do_not() {
        let app = app_with_focus(Focus::LogList);
        let log = page_entries(&app, HelpPage::Log);
        let labels: Vec<&str> = log.iter().map(|e| e.label).collect();
        assert!(labels.contains(&"wrap"), "{labels:?}");
        assert!(labels.contains(&"visual"), "{labels:?}");
        assert!(labels.contains(&"yank msg"), "{labels:?}");
        assert!(labels.contains(&"yank field"), "{labels:?}");
        let overlays = page_entries(&app, HelpPage::Overlays);
        let overlay_labels: Vec<&str> = overlays.iter().map(|e| e.label).collect();
        assert!(
            !overlay_labels.contains(&"wrap") && !overlay_labels.contains(&"visual"),
            "{overlay_labels:?}"
        );
        let session = page_entries(&app, HelpPage::Session);
        assert!(
            session.iter().any(|e| e.label == "bookmarks"),
            "{session:?}"
        );
    }

    #[test]
    fn search_chip_is_ignore_case_substring() {
        let app = app_with_focus(Focus::LogList);
        let hits = search_help_hits(&app, "CHIP");
        assert!(!hits.is_empty(), "expected substring hits for CHIP");
        assert!(hits.iter().any(|h| {
            let text = if let Some(page) = h.page {
                page_doc_lines(&app, page)[h.line].text.clone()
            } else {
                home_doc_lines(&app)[h.line].text.clone()
            };
            text[h.start..h.end].eq_ignore_ascii_case("chip")
        }));
    }

    #[test]
    fn rebound_key_shows_in_status_hints() {
        let mut app = app_with_focus(Focus::LogList);
        app.keymap = crate::keymap::merge_user_toml(
            r#"
[log_list]
move_down = "Down"
"#,
        )
        .unwrap();
        let entries = context_entries(&app);
        assert!(
            entries.iter().any(|e| e.key.contains("Down")),
            "custom move_down must appear: {entries:?}"
        );
    }

    #[test]
    fn catalog_and_loglist_include_command_palette() {
        let app = app_with_focus(Focus::LogList);
        let entries = context_entries(&app);
        assert!(
            entries
                .iter()
                .any(|e| e.key == "C-p" && e.label == "palette"),
            "LogList Active must list C-p palette: {entries:?}"
        );
        let overlay = joined_lines(
            &page_doc_lines(&app, HelpPage::Overlays)
                .into_iter()
                .map(|l| l.line)
                .collect::<Vec<_>>(),
        );
        assert!(
            overlay.contains("open command palette") || overlay.contains("C-p"),
            "Overlays page must mention the palette binding: {overlay}"
        );
        let idle = status_hint_entries(&app);
        let labels: Vec<&str> = idle.iter().map(|e| e.label).collect();
        assert_eq!(labels, ["help", "filter"], "idle status stays two hints");
    }

    #[test]
    fn command_palette_open_blocks_help_and_uses_palette_context() {
        let mut app = app_with_focus(Focus::LogList);
        app.open_command_palette();
        assert!(!help_available(&app));
        assert_eq!(context_kind(&app), ContextKind::CommandPalette);
        let labels: Vec<&str> = status_hint_entries(&app).iter().map(|e| e.label).collect();
        assert!(
            labels.contains(&"close") || labels.contains(&"run"),
            "{labels:?}"
        );
        assert!(!labels.contains(&"help") || labels.len() > 2);
    }

    #[test]
    fn compare_panel_blocks_help_and_uses_compare_context() {
        let mut app = app_with_focus(Focus::LogList);
        let mut row = crate::model::EntryRow::from_line_or_raw("04-02 10:00:00.000  1  1 I T : x");
        row.row_id = 1;
        app.bookmarks
            .try_add(crate::bookmark::Bookmark::from_row(row))
            .unwrap();
        app.bookmark_row_ids.insert(1);
        app.open_compare_panel();
        assert!(!help_available(&app));
        assert_eq!(context_kind(&app), ContextKind::Compare);
        let labels: Vec<&str> = context_entries(&app).iter().map(|e| e.label).collect();
        assert!(labels.contains(&"jump"), "{labels:?}");
        assert!(labels.contains(&"yank"), "{labels:?}");
        assert!(!labels.iter().any(|l| *l == "manage"), "{labels:?}");
    }

    #[test]
    fn pending_m_says_compare_not_manage() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_m = true;
        let labels: Vec<&str> = context_entries(&app).iter().map(|e| e.label).collect();
        assert!(labels.contains(&"compare"), "{labels:?}");
        assert!(!labels.contains(&"manage"), "{labels:?}");
    }

    #[test]
    fn page_max_scroll_keeps_last_line_at_bottom() {
        assert_eq!(page_max_scroll(10, 20), 0);
        assert_eq!(page_max_scroll(25, 10), 15);
        assert_eq!(page_max_scroll(1, 1), 0);
        assert_eq!(page_max_scroll(8, 0), 7);
    }
}
