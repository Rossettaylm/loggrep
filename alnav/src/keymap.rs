//! Configurable TUI keymap: DSL, action registry, merge, and KeymapStore.
//!
//! The Rust registry is the sole authority for defaults; `keymap.toml` deep-merges
//! overrides. Help/status format keys via [`KeymapStore::display`].

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::theme;

/// Action classification for chord trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Arms a pending operator / leader; does not perform work alone.
    Prefix,
    /// Executes when its full binding is completed.
    Leaf,
}

/// Runtime capability filter (independent of whether a key is bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Only active for `-f` file sessions.
    FileOnly,
    /// Only active for `--hdc` / `--adb` live sessions.
    LiveOnly,
}

/// Normalized key stroke (letters stored lowercase; Shift via flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    pub code: StrokeCode,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StrokeCode {
    Char(char),
    Esc,
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
}

impl KeyStroke {
    pub fn char(c: char) -> Self {
        Self {
            code: StrokeCode::Char(c),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty key stroke".into());
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut rest = s;
        loop {
            if let Some(r) = rest.strip_prefix("C-") {
                ctrl = true;
                rest = r;
                continue;
            }
            if let Some(r) = rest.strip_prefix("S-") {
                shift = true;
                rest = r;
                continue;
            }
            if let Some(r) = rest.strip_prefix("M-") {
                alt = true;
                rest = r;
                continue;
            }
            break;
        }
        if rest.is_empty() {
            return Err(format!("incomplete key stroke: {s}"));
        }
        let code = match rest {
            "Space" => StrokeCode::Char(' '),
            "Esc" | "Escape" => StrokeCode::Esc,
            "Enter" | "Return" => StrokeCode::Enter,
            "Tab" => StrokeCode::Tab,
            "BackTab" => StrokeCode::BackTab,
            "Backspace" | "BS" => StrokeCode::Backspace,
            "Delete" | "Del" => StrokeCode::Delete,
            "Up" => StrokeCode::Up,
            "Down" => StrokeCode::Down,
            "Left" => StrokeCode::Left,
            "Right" => StrokeCode::Right,
            "Home" => StrokeCode::Home,
            "End" => StrokeCode::End,
            _ => {
                let mut chars = rest.chars();
                let c = chars.next().ok_or_else(|| format!("bad key: {s}"))?;
                if chars.next().is_some() {
                    return Err(format!("unknown key name: {rest}"));
                }
                if c.is_ascii_uppercase() {
                    return Err(format!(
                        "use S-{} for Shift+letter (bare '{c}' is invalid)",
                        c.to_ascii_lowercase()
                    ));
                }
                StrokeCode::Char(c)
            }
        };
        Ok(Self {
            code,
            ctrl,
            shift,
            alt,
        })
    }

    pub fn format(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("C-");
        }
        if self.alt {
            out.push_str("M-");
        }
        if self.shift {
            out.push_str("S-");
        }
        match self.code {
            StrokeCode::Char(' ') => out.push_str("Space"),
            StrokeCode::Char(c) => out.push(c),
            StrokeCode::Esc => out.push_str("Esc"),
            StrokeCode::Enter => out.push_str("Enter"),
            StrokeCode::Tab => out.push_str("Tab"),
            StrokeCode::BackTab => out.push_str("BackTab"),
            StrokeCode::Backspace => out.push_str("Backspace"),
            StrokeCode::Delete => out.push_str("Delete"),
            StrokeCode::Up => out.push_str("Up"),
            StrokeCode::Down => out.push_str("Down"),
            StrokeCode::Left => out.push_str("Left"),
            StrokeCode::Right => out.push_str("Right"),
            StrokeCode::Home => out.push_str("Home"),
            StrokeCode::End => out.push_str("End"),
        }
        out
    }

    /// Compact UI form: Shift+letter → `J`, otherwise same as [`Self::format`].
    pub fn format_ui(&self) -> String {
        if self.shift && !self.ctrl && !self.alt {
            if let StrokeCode::Char(c) = self.code {
                if c.is_ascii_lowercase() {
                    return c.to_ascii_uppercase().to_string();
                }
            }
        }
        self.format()
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

/// One or more strokes forming a binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Binding {
    pub strokes: Vec<KeyStroke>,
}

impl Binding {
    pub fn parse_str(s: &str) -> Result<Self, String> {
        Ok(Self {
            strokes: vec![KeyStroke::parse(s)?],
        })
    }

    /// No default key. `--init` serializes as `""` (unbind).
    pub fn unbound() -> Self {
        Self {
            strokes: Vec::new(),
        }
    }

    pub fn format(&self) -> String {
        self.strokes
            .iter()
            .map(KeyStroke::format)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Compact chord for L1 (e.g. `ma`, `mm`).
    pub fn format_compact(&self) -> String {
        if self.strokes.len() == 1 {
            return self.strokes[0].format_ui();
        }
        self.strokes
            .iter()
            .map(|s| match s.code {
                StrokeCode::Char(' ') => "Space".to_string(),
                StrokeCode::Char(c) if !s.ctrl && !s.alt && !s.shift => c.to_string(),
                _ => s.format_ui(),
            })
            .collect()
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

/// Convert a bare [`KeyCode`] (Normal-mode path) into a stroke.
pub fn stroke_from_keycode(code: KeyCode) -> Option<KeyStroke> {
    match code {
        KeyCode::Char(c) if c.is_ascii_uppercase() => Some(KeyStroke {
            code: StrokeCode::Char(c.to_ascii_lowercase()),
            ctrl: false,
            shift: true,
            alt: false,
        }),
        KeyCode::Char(c) => Some(KeyStroke::char(c)),
        KeyCode::Esc => Some(KeyStroke {
            code: StrokeCode::Esc,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Enter => Some(KeyStroke {
            code: StrokeCode::Enter,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Tab => Some(KeyStroke {
            code: StrokeCode::Tab,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::BackTab => Some(KeyStroke {
            code: StrokeCode::BackTab,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Backspace => Some(KeyStroke {
            code: StrokeCode::Backspace,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Delete => Some(KeyStroke {
            code: StrokeCode::Delete,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Up => Some(KeyStroke {
            code: StrokeCode::Up,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Down => Some(KeyStroke {
            code: StrokeCode::Down,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Left => Some(KeyStroke {
            code: StrokeCode::Left,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Right => Some(KeyStroke {
            code: StrokeCode::Right,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Home => Some(KeyStroke {
            code: StrokeCode::Home,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::End => Some(KeyStroke {
            code: StrokeCode::End,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        _ => None,
    }
}

/// Convert a full [`KeyEvent`] into a stroke.
pub fn stroke_from_event(key: KeyEvent) -> Option<KeyStroke> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift_mod = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char(c) => {
            let (ch, shift) = if c.is_ascii_uppercase() {
                (c.to_ascii_lowercase(), true)
            } else {
                (c, shift_mod)
            };
            Some(KeyStroke {
                code: StrokeCode::Char(ch),
                ctrl,
                shift,
                alt,
            })
        }
        other => {
            let mut s = stroke_from_keycode(other)?;
            s.ctrl = ctrl;
            s.alt = alt;
            if shift_mod {
                s.shift = true;
            }
            Some(s)
        }
    }
}

/// TOML section / runtime key context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyContext {
    Bookmark,
    ChipField,
    Confirm,
    Detail,
    Global,
    Help,
    HighlightModal,
    Input,
    Leader,
    Lock,
    LogList,
    Open,
    Picker,
    Strip,
    StripD,
    Time,
    TimePanel,
    HistPanel,
    Visual,
    Yank,
    CommandPalette,
}

impl KeyContext {
    pub fn as_toml(self) -> &'static str {
        match self {
            Self::Bookmark => "bookmark",
            Self::ChipField => "chip_field",
            Self::Confirm => "confirm",
            Self::Detail => "detail",
            Self::Global => "global",
            Self::Help => "help",
            Self::HighlightModal => "highlight_modal",
            Self::Input => "input",
            Self::Leader => "leader",
            Self::Lock => "lock",
            Self::LogList => "log_list",
            Self::Open => "open",
            Self::Picker => "picker",
            Self::Strip => "strip",
            Self::StripD => "strip_d",
            Self::Time => "time",
            Self::TimePanel => "time_panel",
            Self::HistPanel => "hist_panel",
            Self::Visual => "visual",
            Self::Yank => "yank",
            Self::CommandPalette => "command_palette",
        }
    }
    pub fn from_toml(s: &str) -> Option<Self> {
        match s {
            "bookmark" => Some(Self::Bookmark),
            "chip_field" => Some(Self::ChipField),
            "confirm" => Some(Self::Confirm),
            "detail" => Some(Self::Detail),
            "global" => Some(Self::Global),
            "help" => Some(Self::Help),
            "highlight_modal" => Some(Self::HighlightModal),
            "input" => Some(Self::Input),
            "leader" => Some(Self::Leader),
            "lock" => Some(Self::Lock),
            "log_list" => Some(Self::LogList),
            "open" => Some(Self::Open),
            "picker" => Some(Self::Picker),
            "strip" => Some(Self::Strip),
            "strip_d" => Some(Self::StripD),
            "time" => Some(Self::Time),
            "time_panel" => Some(Self::TimePanel),
            "hist_panel" => Some(Self::HistPanel),
            "visual" => Some(Self::Visual),
            "yank" => Some(Self::Yank),
            "command_palette" => Some(Self::CommandPalette),
            _ => None,
        }
    }
}

/// Stable action identity (every keyboard action registers here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    GlobalQuit,
    GlobalFocusNext,
    GlobalFocusPrev,
    GlobalFocusFilter,
    GlobalFocusExclude,
    GlobalFocusHighlight,
    GlobalFocusLog,
    GlobalFocusInput,
    GlobalFilterNew,
    GlobalHighlightNew,
    GlobalHighlightAdd,
    GlobalExcludeNew,
    GlobalOpenHelp,
    GlobalCommandPalette,
    LogListMoveDown,
    LogListMoveUp,
    LogListJumpDown,
    LogListJumpUp,
    LogListJumpTop,
    LogListJumpBottom,
    LogListResumeFollow,
    LogListNextMatch,
    LogListPrevMatch,
    LogListNextSevere,
    LogListPrevSevere,
    LogListDetailFields,
    LogListDetailPretty,
    LogListVisualLine,
    LogListYankMsgLine,
    LogListClearLive,
    LogListPageDown,
    LogListPageUp,
    LogListLeader,
    LogListBookmark,
    LogListChip,
    LogListExcludeChip,
    LogListYank,
    LogListLock,
    LogListTime,
    LogListWrapToggle,
    LeaderManage,
    ClearAllRules,
    LeaderPresetSave,
    LeaderPresetOpen,
    LeaderSummary,
    LeaderCancel,
    BookmarkAdd,
    BookmarkRemove,
    BookmarkManage,
    BookmarkCancel,
    LockPid,
    LockTid,
    LockViewHighlight,
    LockViewSevere,
    LockClear,
    LockCancel,
    OpenFile,
    OpenStream,
    TimeSet,
    TimeClear,
    TimeCancel,
    TimeHistogram,
    HistPanelPrev,
    HistPanelNext,
    HistPanelJumpDown,
    HistPanelJumpUp,
    HistPanelJumpTop,
    HistPanelJumpBottom,
    HistPanelZoomIn,
    HistPanelZoomOut,
    HistPanelSubmit,
    HistPanelApplyWindow,
    HistPanelCancel,
    ChipFieldTag,
    ChipFieldMsg,
    ChipFieldPkg,
    ChipFieldPid,
    ChipFieldTid,
    ChipFieldLevel,
    ChipFieldCancel,
    YankCli,
    YankTag,
    YankMsg,
    YankPkg,
    YankPid,
    YankTid,
    YankLevel,
    YankRaw,
    YankLine,
    YankTime,
    YankCancel,
    StripDDelete,
    StripDDisable,
    StripDCancel,
    StripPendingD,
    StripPrevGroup,
    StripNextGroup,
    StripResumeFollow,
    StripOpenHelp,
    StripFocusNext,
    VisualMoveDown,
    VisualMoveUp,
    VisualJumpDown,
    VisualJumpUp,
    VisualYankRaw,
    VisualYankMsg,
    VisualCancel,
    HelpClose,
    HelpToggle,
    HelpScrollDown,
    HelpScrollUp,
    HelpJumpDown,
    HelpJumpUp,
    HelpTop,
    HelpBottom,
    HelpBack,
    HelpBackAlt,
    HelpSearch,
    HelpSearchNext,
    HelpSearchPrev,
    HelpSubmit,
    PickerSubmit,
    PickerUp,
    PickerDown,
    PickerMulti,
    PickerEdit,
    PickerDelete,
    PickerDeleteAlt,
    PickerClose,
    ConfirmYes,
    ConfirmYesEnter,
    ConfirmNo,
    ConfirmCancel,
    DetailCloseFields,
    DetailSwap,
    DetailChip,
    DetailExclude,
    DetailMoveDown,
    DetailMoveUp,
    DetailClose,
    TimePanelNext,
    TimePanelSubmit,
    TimePanelDateUp,
    TimePanelDateDown,
    TimePanelCancel,
    InputDraftSpace,
    InputCommit,
    InputToggleExclude,
    InputCancel,
    HighlightModalDraftSpace,
    HighlightModalConfirm,
    HighlightModalConfirmTab,
    HighlightModalCancel,
    PaletteSubmit,
    PaletteUp,
    PaletteDown,
    PaletteClose,
}

/// Metadata for one registered action (defaults live here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMeta {
    pub id: ActionId,
    pub context: KeyContext,
    pub toml_key: &'static str,
    pub default: Binding,
    pub kind: ActionKind,
    pub capabilities: &'static [Capability],
    pub label: &'static str,
    pub detail: &'static str,
    /// Intent-command catalog (command palette). Unused when `in_palette` is false.
    pub in_palette: bool,
    pub palette_title: &'static str,
    pub palette_icon: &'static str,
}

impl ActionMeta {
    pub fn allowed(&self, file_mode: bool) -> bool {
        for c in self.capabilities {
            match c {
                Capability::FileOnly if !file_mode => return false,
                Capability::LiveOnly if file_mode => return false,
                _ => {}
            }
        }
        true
    }

    pub fn with_palette(mut self, title: &'static str, icon: &'static str) -> Self {
        self.in_palette = true;
        self.palette_title = title;
        self.palette_icon = icon;
        self
    }
}

impl ActionId {
    pub const ALL: &'static [ActionId] = &[
        Self::GlobalQuit,
        Self::GlobalFocusNext,
        Self::GlobalFocusPrev,
        Self::GlobalFocusFilter,
        Self::GlobalFocusExclude,
        Self::GlobalFocusHighlight,
        Self::GlobalFocusLog,
        Self::GlobalFocusInput,
        Self::GlobalFilterNew,
        Self::GlobalHighlightNew,
        Self::GlobalHighlightAdd,
        Self::GlobalExcludeNew,
        Self::GlobalOpenHelp,
        Self::GlobalCommandPalette,
        Self::LogListMoveDown,
        Self::LogListMoveUp,
        Self::LogListJumpDown,
        Self::LogListJumpUp,
        Self::LogListJumpTop,
        Self::LogListJumpBottom,
        Self::LogListResumeFollow,
        Self::LogListNextMatch,
        Self::LogListPrevMatch,
        Self::LogListNextSevere,
        Self::LogListPrevSevere,
        Self::LogListDetailFields,
        Self::LogListDetailPretty,
        Self::LogListVisualLine,
        Self::LogListYankMsgLine,
        Self::LogListClearLive,
        Self::LogListPageDown,
        Self::LogListPageUp,
        Self::LogListLeader,
        Self::LogListBookmark,
        Self::LogListChip,
        Self::LogListExcludeChip,
        Self::LogListYank,
        Self::LogListLock,
        Self::LogListTime,
        Self::LogListWrapToggle,
        Self::LeaderManage,
        Self::ClearAllRules,
        Self::LeaderPresetSave,
        Self::LeaderPresetOpen,
        Self::LeaderSummary,
        Self::LeaderCancel,
        Self::BookmarkAdd,
        Self::BookmarkRemove,
        Self::BookmarkManage,
        Self::BookmarkCancel,
        Self::LockPid,
        Self::LockTid,
        Self::LockViewHighlight,
        Self::LockViewSevere,
        Self::LockClear,
        Self::LockCancel,
        Self::OpenFile,
        Self::OpenStream,
        Self::TimeSet,
        Self::TimeClear,
        Self::TimeCancel,
        Self::TimeHistogram,
        Self::HistPanelPrev,
        Self::HistPanelNext,
        Self::HistPanelJumpDown,
        Self::HistPanelJumpUp,
        Self::HistPanelJumpTop,
        Self::HistPanelJumpBottom,
        Self::HistPanelZoomIn,
        Self::HistPanelZoomOut,
        Self::HistPanelSubmit,
        Self::HistPanelApplyWindow,
        Self::HistPanelCancel,
        Self::ChipFieldTag,
        Self::ChipFieldMsg,
        Self::ChipFieldPkg,
        Self::ChipFieldPid,
        Self::ChipFieldTid,
        Self::ChipFieldLevel,
        Self::ChipFieldCancel,
        Self::YankCli,
        Self::YankTag,
        Self::YankMsg,
        Self::YankPkg,
        Self::YankPid,
        Self::YankTid,
        Self::YankLevel,
        Self::YankRaw,
        Self::YankLine,
        Self::YankTime,
        Self::YankCancel,
        Self::StripDDelete,
        Self::StripDDisable,
        Self::StripDCancel,
        Self::StripPendingD,
        Self::StripPrevGroup,
        Self::StripNextGroup,
        Self::StripResumeFollow,
        Self::StripOpenHelp,
        Self::StripFocusNext,
        Self::VisualMoveDown,
        Self::VisualMoveUp,
        Self::VisualJumpDown,
        Self::VisualJumpUp,
        Self::VisualYankRaw,
        Self::VisualYankMsg,
        Self::VisualCancel,
        Self::HelpClose,
        Self::HelpToggle,
        Self::HelpScrollDown,
        Self::HelpScrollUp,
        Self::HelpJumpDown,
        Self::HelpJumpUp,
        Self::HelpTop,
        Self::HelpBottom,
        Self::HelpBack,
        Self::HelpBackAlt,
        Self::HelpSearch,
        Self::HelpSearchNext,
        Self::HelpSearchPrev,
        Self::HelpSubmit,
        Self::PickerSubmit,
        Self::PickerUp,
        Self::PickerDown,
        Self::PickerMulti,
        Self::PickerEdit,
        Self::PickerDelete,
        Self::PickerDeleteAlt,
        Self::PickerClose,
        Self::ConfirmYes,
        Self::ConfirmYesEnter,
        Self::ConfirmNo,
        Self::ConfirmCancel,
        Self::DetailCloseFields,
        Self::DetailSwap,
        Self::DetailChip,
        Self::DetailExclude,
        Self::DetailMoveDown,
        Self::DetailMoveUp,
        Self::DetailClose,
        Self::TimePanelNext,
        Self::TimePanelSubmit,
        Self::TimePanelDateUp,
        Self::TimePanelDateDown,
        Self::TimePanelCancel,
        Self::InputDraftSpace,
        Self::InputCommit,
        Self::InputToggleExclude,
        Self::InputCancel,
        Self::HighlightModalDraftSpace,
        Self::HighlightModalConfirm,
        Self::HighlightModalConfirmTab,
        Self::HighlightModalCancel,
        Self::PaletteSubmit,
        Self::PaletteUp,
        Self::PaletteDown,
        Self::PaletteClose,
    ];

    pub fn meta(self) -> ActionMeta {
        match self {
            Self::GlobalQuit => ActionMeta {
                id: Self::GlobalQuit,
                context: KeyContext::Global,
                toml_key: "quit",
                default: Binding::parse_str("q").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "quit",
                detail: "quit the application",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Quit", theme::GLYPH_QUIT),
            Self::GlobalFocusNext => ActionMeta {
                id: Self::GlobalFocusNext,
                context: KeyContext::Global,
                toml_key: "focus_next",
                default: Binding::parse_str("Tab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "focus",
                detail: "next focus region",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalFocusPrev => ActionMeta {
                id: Self::GlobalFocusPrev,
                context: KeyContext::Global,
                toml_key: "focus_prev",
                default: Binding::parse_str("BackTab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "focus",
                detail: "previous focus region",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalFocusFilter => ActionMeta {
                id: Self::GlobalFocusFilter,
                context: KeyContext::Global,
                toml_key: "focus_filter",
                default: Binding::parse_str("1").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "filter",
                detail: "focus filter strip",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalFocusExclude => ActionMeta {
                id: Self::GlobalFocusExclude,
                context: KeyContext::Global,
                toml_key: "focus_exclude",
                default: Binding::parse_str("2").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "exclude",
                detail: "focus exclude strip",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalFocusHighlight => ActionMeta {
                id: Self::GlobalFocusHighlight,
                context: KeyContext::Global,
                toml_key: "focus_highlight",
                default: Binding::parse_str("3").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "highlight",
                detail: "focus highlight strip",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalFocusLog => ActionMeta {
                id: Self::GlobalFocusLog,
                context: KeyContext::Global,
                toml_key: "focus_log",
                default: Binding::parse_str("4").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "log",
                detail: "focus log list",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalFocusInput => ActionMeta {
                id: Self::GlobalFocusInput,
                context: KeyContext::Global,
                toml_key: "focus_input",
                default: Binding::parse_str("5").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "input",
                detail: "open unified manage",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalFilterNew => ActionMeta {
                id: Self::GlobalFilterNew,
                context: KeyContext::Global,
                toml_key: "filter_new",
                default: Binding::parse_str(";").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "filter",
                detail: "open filter picker in new mode",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Add Filter", theme::GLYPH_TITLE_FILTER),
            Self::GlobalHighlightNew => ActionMeta {
                id: Self::GlobalHighlightNew,
                context: KeyContext::Global,
                toml_key: "highlight_new",
                default: Binding::parse_str("/").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "highlight",
                detail: "find or create a highlight and jump to the first hit",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Find Highlight", theme::GLYPH_TITLE_HIGHLIGHT),
            Self::GlobalHighlightAdd => ActionMeta {
                id: Self::GlobalHighlightAdd,
                context: KeyContext::Global,
                toml_key: "highlight_add",
                default: Binding::unbound(),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "add highlight",
                detail: "open highlight picker in new mode",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Add Highlight", theme::GLYPH_TITLE_HIGHLIGHT),
            Self::GlobalExcludeNew => ActionMeta {
                id: Self::GlobalExcludeNew,
                context: KeyContext::Global,
                toml_key: "exclude_new",
                default: Binding::parse_str("`").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "exclude",
                detail: "open exclude picker in new mode",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Add Exclude", theme::GLYPH_TITLE_EXCLUDE),
            Self::GlobalOpenHelp => ActionMeta {
                id: Self::GlobalOpenHelp,
                context: KeyContext::Global,
                toml_key: "open_help",
                default: Binding::parse_str("?").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "help",
                detail: "open help panel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Open Help", theme::GLYPH_HELP),
            Self::LogListMoveDown => ActionMeta {
                id: Self::LogListMoveDown,
                context: KeyContext::LogList,
                toml_key: "move_down",
                default: Binding::parse_str("j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "move",
                detail: "move cursor down",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListMoveUp => ActionMeta {
                id: Self::LogListMoveUp,
                context: KeyContext::LogList,
                toml_key: "move_up",
                default: Binding::parse_str("k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "move",
                detail: "move cursor up",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListJumpDown => ActionMeta {
                id: Self::LogListJumpDown,
                context: KeyContext::LogList,
                toml_key: "jump_down",
                default: Binding::parse_str("S-j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "jump",
                detail: "move down fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListJumpUp => ActionMeta {
                id: Self::LogListJumpUp,
                context: KeyContext::LogList,
                toml_key: "jump_up",
                default: Binding::parse_str("S-k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "jump",
                detail: "move up fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListJumpTop => ActionMeta {
                id: Self::LogListJumpTop,
                context: KeyContext::LogList,
                toml_key: "jump_top",
                default: Binding::parse_str("g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "top",
                detail: "jump to top",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListJumpBottom => ActionMeta {
                id: Self::LogListJumpBottom,
                context: KeyContext::LogList,
                toml_key: "jump_bottom",
                default: Binding::parse_str("S-g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "bottom",
                detail: "jump to bottom and resume follow",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListResumeFollow => ActionMeta {
                id: Self::LogListResumeFollow,
                context: KeyContext::LogList,
                toml_key: "resume_follow",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "follow",
                detail: "resume following and pin to bottom",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Resume Following", theme::GLYPH_FOLLOWING),
            Self::LogListNextMatch => ActionMeta {
                id: Self::LogListNextMatch,
                context: KeyContext::LogList,
                toml_key: "next_match",
                default: Binding::parse_str("n").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "next",
                detail: "next highlight match",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListPrevMatch => ActionMeta {
                id: Self::LogListPrevMatch,
                context: KeyContext::LogList,
                toml_key: "prev_match",
                default: Binding::parse_str("S-n").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "prev",
                detail: "previous highlight match",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListNextSevere => ActionMeta {
                id: Self::LogListNextSevere,
                context: KeyContext::LogList,
                toml_key: "next_severe",
                default: Binding::parse_str("e").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "error",
                detail: "next severe line",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListPrevSevere => ActionMeta {
                id: Self::LogListPrevSevere,
                context: KeyContext::LogList,
                toml_key: "prev_severe",
                default: Binding::parse_str("S-e").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "error",
                detail: "previous severe line",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListDetailFields => ActionMeta {
                id: Self::LogListDetailFields,
                context: KeyContext::LogList,
                toml_key: "detail_fields",
                default: Binding::parse_str("p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "detail",
                detail: "toggle fields overlay",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Show Fields", theme::GLYPH_VIEW_FOCUS),
            Self::LogListDetailPretty => ActionMeta {
                id: Self::LogListDetailPretty,
                context: KeyContext::LogList,
                toml_key: "detail_pretty",
                default: Binding::parse_str("S-p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "pretty",
                detail: "toggle pretty / swap overlay (crash rows show structured detail)",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Show Pretty", theme::GLYPH_VIEW_FOCUS),
            Self::LogListVisualLine => ActionMeta {
                id: Self::LogListVisualLine,
                context: KeyContext::LogList,
                toml_key: "visual_line",
                default: Binding::parse_str("S-v").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "visual",
                detail: "enter visual line mode",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListYankMsgLine => ActionMeta {
                id: Self::LogListYankMsgLine,
                context: KeyContext::LogList,
                toml_key: "yank_msg_line",
                default: Binding::parse_str("S-y").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "yank",
                detail: "yank message of current line",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Yank Message", theme::GLYPH_FIELD_MSG),
            Self::LogListClearLive => ActionMeta {
                id: Self::LogListClearLive,
                context: KeyContext::LogList,
                toml_key: "clear_live",
                default: Binding::parse_str("C-l").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::LiveOnly],
                label: "clear",
                detail: "clear buffered live logs",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Clear Live Buffer", theme::GLYPH_DISCONNECT),
            Self::LogListPageDown => ActionMeta {
                id: Self::LogListPageDown,
                context: KeyContext::LogList,
                toml_key: "page_down",
                default: Binding::parse_str("C-d").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "page",
                detail: "page down",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListPageUp => ActionMeta {
                id: Self::LogListPageUp,
                context: KeyContext::LogList,
                toml_key: "page_up",
                default: Binding::parse_str("C-u").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "page",
                detail: "page up",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListLeader => ActionMeta {
                id: Self::LogListLeader,
                context: KeyContext::LogList,
                toml_key: "leader",
                default: Binding::parse_str("Space").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "menu",
                detail: "leader prefix",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListBookmark => ActionMeta {
                id: Self::LogListBookmark,
                context: KeyContext::LogList,
                toml_key: "bookmark",
                default: Binding::parse_str("m").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "mark",
                detail: "bookmark operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListChip => ActionMeta {
                id: Self::LogListChip,
                context: KeyContext::LogList,
                toml_key: "chip",
                default: Binding::parse_str("c").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "chip",
                detail: "filter chip from row",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListExcludeChip => ActionMeta {
                id: Self::LogListExcludeChip,
                context: KeyContext::LogList,
                toml_key: "exclude_chip",
                default: Binding::parse_str("S-c").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "exclude",
                detail: "exclude chip from row",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListYank => ActionMeta {
                id: Self::LogListYank,
                context: KeyContext::LogList,
                toml_key: "yank",
                default: Binding::parse_str("y").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "yank",
                detail: "yank operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListLock => ActionMeta {
                id: Self::LogListLock,
                context: KeyContext::LogList,
                toml_key: "lock",
                default: Binding::parse_str("f").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "focus",
                detail: "lock / view focus operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListTime => ActionMeta {
                id: Self::LogListTime,
                context: KeyContext::LogList,
                toml_key: "time",
                default: Binding::parse_str("t").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[Capability::FileOnly],
                label: "time",
                detail: "time window operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LogListWrapToggle => ActionMeta {
                id: Self::LogListWrapToggle,
                context: KeyContext::LogList,
                toml_key: "wrap_toggle",
                default: Binding::parse_str("w").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "wrap",
                detail: "toggle multi-line / single-line collapsed view",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Toggle Wrap", theme::GLYPH_TITLE_LOG),
            Self::LeaderManage => ActionMeta {
                id: Self::LeaderManage,
                context: KeyContext::Leader,
                toml_key: "manage",
                default: Binding::parse_str("Space").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "manage",
                detail: "open unified manage panel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Manage Rules", theme::GLYPH_MODE_MANAGE),
            Self::ClearAllRules => ActionMeta {
                id: Self::ClearAllRules,
                context: KeyContext::Picker,
                toml_key: "clear_all_rules",
                default: Binding::parse_str("C-k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "clear",
                detail: "clear all filter / highlight / exclude rules",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Clear All Rules", theme::GLYPH_TITLE_EXCLUDE),
            Self::LeaderPresetSave => ActionMeta {
                id: Self::LeaderPresetSave,
                context: KeyContext::Global,
                toml_key: "preset_save",
                default: Binding::parse_str("C-s").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "save",
                detail: "save filter preset",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Save Preset", theme::GLYPH_MODE_NEW),
            Self::LeaderPresetOpen => ActionMeta {
                id: Self::LeaderPresetOpen,
                context: KeyContext::Global,
                toml_key: "preset_open",
                default: Binding::parse_str("C-o").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "open",
                detail: "open filter preset",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Open Preset", theme::GLYPH_SOURCE_DIR),
            Self::LeaderSummary => ActionMeta {
                id: Self::LeaderSummary,
                context: KeyContext::Leader,
                toml_key: "summary",
                default: Binding::parse_str("i").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "stats",
                detail: "open summary panel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Show Summary", theme::GLYPH_TITLE_DASHBOARD),
            Self::LeaderCancel => ActionMeta {
                id: Self::LeaderCancel,
                context: KeyContext::Leader,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel leader",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::BookmarkAdd => ActionMeta {
                id: Self::BookmarkAdd,
                context: KeyContext::Bookmark,
                toml_key: "add",
                default: Binding::parse_str("a").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "add",
                detail: "bookmark current row",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Add Bookmark", theme::GLYPH_BOOKMARK),
            Self::BookmarkRemove => ActionMeta {
                id: Self::BookmarkRemove,
                context: KeyContext::Bookmark,
                toml_key: "remove",
                default: Binding::parse_str("d").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "delete",
                detail: "remove bookmark on current row",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Remove Bookmark", theme::GLYPH_BOOKMARK),
            Self::BookmarkManage => ActionMeta {
                id: Self::BookmarkManage,
                context: KeyContext::Bookmark,
                toml_key: "manage",
                default: Binding::parse_str("m").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "compare",
                detail: "open bookmark compare panel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Open Compare Panel", theme::GLYPH_BOOKMARK),
            Self::BookmarkCancel => ActionMeta {
                id: Self::BookmarkCancel,
                context: KeyContext::Bookmark,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel bookmark operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::LockPid => ActionMeta {
                id: Self::LockPid,
                context: KeyContext::Lock,
                toml_key: "pid",
                default: Binding::parse_str("p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "pid",
                detail: "lock to pid",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Lock PID", theme::GLYPH_LOCK),
            Self::LockTid => ActionMeta {
                id: Self::LockTid,
                context: KeyContext::Lock,
                toml_key: "tid",
                default: Binding::parse_str("t").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "tid",
                detail: "lock to tid",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Lock TID", theme::GLYPH_LOCK),
            Self::LockViewHighlight => ActionMeta {
                id: Self::LockViewHighlight,
                context: KeyContext::Lock,
                toml_key: "view_highlight",
                default: Binding::parse_str("h").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "hl",
                detail: "toggle highlight-only view",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("View Focus Highlight", theme::GLYPH_VIEW_FOCUS),
            Self::LockViewSevere => ActionMeta {
                id: Self::LockViewSevere,
                context: KeyContext::Lock,
                toml_key: "view_severe",
                default: Binding::parse_str("e").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "err",
                detail: "toggle severe-only view",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("View Focus Severe", theme::GLYPH_CRASH),
            Self::LockClear => ActionMeta {
                id: Self::LockClear,
                context: KeyContext::Lock,
                toml_key: "clear",
                default: Binding::parse_str("u").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "clear",
                detail: "clear session lock",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Clear Lock", theme::GLYPH_LOCK),
            Self::LockCancel => ActionMeta {
                id: Self::LockCancel,
                context: KeyContext::Lock,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel lock operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::OpenFile => ActionMeta {
                id: Self::OpenFile,
                context: KeyContext::Global,
                toml_key: "open_file",
                default: Binding::parse_str("C-f").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "file",
                detail: "open or switch to a file",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Open File", theme::GLYPH_SOURCE_OPEN_FILE),
            Self::OpenStream => ActionMeta {
                id: Self::OpenStream,
                context: KeyContext::Global,
                toml_key: "open_stream",
                default: Binding::parse_str("C-g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "stream",
                detail: "open or switch to hdc/adb",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Open Stream", theme::GLYPH_SOURCE_HDC),
            Self::TimeSet => ActionMeta {
                id: Self::TimeSet,
                context: KeyContext::Time,
                toml_key: "set",
                default: Binding::parse_str("t").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "set",
                detail: "open time window panel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Set Time Window", theme::GLYPH_TIME),
            Self::TimeClear => ActionMeta {
                id: Self::TimeClear,
                context: KeyContext::Time,
                toml_key: "clear",
                default: Binding::parse_str("u").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "clear",
                detail: "clear time window",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Clear Time Window", theme::GLYPH_TIME),
            Self::TimeCancel => ActionMeta {
                id: Self::TimeCancel,
                context: KeyContext::Time,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "cancel",
                detail: "cancel time operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::TimeHistogram => ActionMeta {
                id: Self::TimeHistogram,
                context: KeyContext::Time,
                toml_key: "histogram",
                default: Binding::parse_str("h").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "hist",
                detail: "open time histogram",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Time Histogram", theme::GLYPH_TIME),
            Self::HistPanelPrev => ActionMeta {
                id: Self::HistPanelPrev,
                context: KeyContext::HistPanel,
                toml_key: "prev",
                default: Binding::parse_str("k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "prev",
                detail: "previous bucket",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelNext => ActionMeta {
                id: Self::HistPanelNext,
                context: KeyContext::HistPanel,
                toml_key: "next",
                default: Binding::parse_str("j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "next",
                detail: "next bucket",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelJumpDown => ActionMeta {
                id: Self::HistPanelJumpDown,
                context: KeyContext::HistPanel,
                toml_key: "jump_down",
                default: Binding::parse_str("S-j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "jump",
                detail: "move down fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelJumpUp => ActionMeta {
                id: Self::HistPanelJumpUp,
                context: KeyContext::HistPanel,
                toml_key: "jump_up",
                default: Binding::parse_str("S-k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "jump",
                detail: "move up fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelJumpTop => ActionMeta {
                id: Self::HistPanelJumpTop,
                context: KeyContext::HistPanel,
                toml_key: "jump_top",
                default: Binding::parse_str("g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "top",
                detail: "jump to first bucket",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelJumpBottom => ActionMeta {
                id: Self::HistPanelJumpBottom,
                context: KeyContext::HistPanel,
                toml_key: "jump_bottom",
                default: Binding::parse_str("S-g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "bottom",
                detail: "jump to last bucket",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelZoomIn => ActionMeta {
                id: Self::HistPanelZoomIn,
                context: KeyContext::HistPanel,
                toml_key: "zoom_in",
                default: Binding::parse_str("BackTab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "finer",
                detail: "cycle finer buckets (5m→1m→10s)",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelZoomOut => ActionMeta {
                id: Self::HistPanelZoomOut,
                context: KeyContext::HistPanel,
                toml_key: "zoom_out",
                default: Binding::parse_str("Tab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "coarser",
                detail: "cycle coarser buckets (10s→1m→5m)",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelSubmit => ActionMeta {
                id: Self::HistPanelSubmit,
                context: KeyContext::HistPanel,
                toml_key: "submit",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "jump",
                detail: "jump to bucket",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelApplyWindow => ActionMeta {
                id: Self::HistPanelApplyWindow,
                context: KeyContext::HistPanel,
                toml_key: "apply_window",
                default: Binding::parse_str("s").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "window",
                detail: "set time window from bucket",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HistPanelCancel => ActionMeta {
                id: Self::HistPanelCancel,
                context: KeyContext::HistPanel,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[Capability::FileOnly],
                label: "close",
                detail: "close histogram",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ChipFieldTag => ActionMeta {
                id: Self::ChipFieldTag,
                context: KeyContext::ChipField,
                toml_key: "tag",
                default: Binding::parse_str("t").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "tag",
                detail: "select tag field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ChipFieldMsg => ActionMeta {
                id: Self::ChipFieldMsg,
                context: KeyContext::ChipField,
                toml_key: "msg",
                default: Binding::parse_str("m").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "msg",
                detail: "select msg field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ChipFieldPkg => ActionMeta {
                id: Self::ChipFieldPkg,
                context: KeyContext::ChipField,
                toml_key: "pkg",
                default: Binding::parse_str("g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "pkg",
                detail: "select pkg field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ChipFieldPid => ActionMeta {
                id: Self::ChipFieldPid,
                context: KeyContext::ChipField,
                toml_key: "pid",
                default: Binding::parse_str("p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "pid",
                detail: "select pid field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ChipFieldTid => ActionMeta {
                id: Self::ChipFieldTid,
                context: KeyContext::ChipField,
                toml_key: "tid",
                default: Binding::parse_str("S-t").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "tid",
                detail: "select tid field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ChipFieldLevel => ActionMeta {
                id: Self::ChipFieldLevel,
                context: KeyContext::ChipField,
                toml_key: "level",
                default: Binding::parse_str("l").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "level",
                detail: "select level field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ChipFieldCancel => ActionMeta {
                id: Self::ChipFieldCancel,
                context: KeyContext::ChipField,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "select cancel field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankCli => ActionMeta {
                id: Self::YankCli,
                context: KeyContext::Yank,
                toml_key: "cli",
                default: Binding::parse_str("c").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cli",
                detail: "yank filters as alnav grep CLI",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Yank CLI", theme::GLYPH_TITLE_LOG),
            Self::YankTag => ActionMeta {
                id: Self::YankTag,
                context: KeyContext::Yank,
                toml_key: "tag",
                default: Binding::parse_str("t").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "tag",
                detail: "yank tag",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankMsg => ActionMeta {
                id: Self::YankMsg,
                context: KeyContext::Yank,
                toml_key: "msg",
                default: Binding::parse_str("m").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "msg",
                detail: "yank msg tokens",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankPkg => ActionMeta {
                id: Self::YankPkg,
                context: KeyContext::Yank,
                toml_key: "pkg",
                default: Binding::parse_str("g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "pkg",
                detail: "yank package",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankPid => ActionMeta {
                id: Self::YankPid,
                context: KeyContext::Yank,
                toml_key: "pid",
                default: Binding::parse_str("p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "pid",
                detail: "yank pid",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankTid => ActionMeta {
                id: Self::YankTid,
                context: KeyContext::Yank,
                toml_key: "tid",
                default: Binding::parse_str("S-t").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "tid",
                detail: "yank tid",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankLevel => ActionMeta {
                id: Self::YankLevel,
                context: KeyContext::Yank,
                toml_key: "level",
                default: Binding::parse_str("l").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "level",
                detail: "yank level",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankRaw => ActionMeta {
                id: Self::YankRaw,
                context: KeyContext::Yank,
                toml_key: "raw",
                default: Binding::parse_str("r").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "raw",
                detail: "yank raw line",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankLine => ActionMeta {
                id: Self::YankLine,
                context: KeyContext::Yank,
                toml_key: "line",
                default: Binding::parse_str("y").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "line",
                detail: "yank formatted line",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankTime => ActionMeta {
                id: Self::YankTime,
                context: KeyContext::Yank,
                toml_key: "time",
                default: Binding::parse_str("s").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "time",
                detail: "yank timestamp",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::YankCancel => ActionMeta {
                id: Self::YankCancel,
                context: KeyContext::Yank,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel yank operator",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::StripDDelete => ActionMeta {
                id: Self::StripDDelete,
                context: KeyContext::StripD,
                toml_key: "delete",
                default: Binding::parse_str("d").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "delete",
                detail: "delete selected strip group",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Delete Selected Group", theme::GLYPH_TITLE_EXCLUDE),
            Self::StripDDisable => ActionMeta {
                id: Self::StripDDisable,
                context: KeyContext::StripD,
                toml_key: "disable",
                default: Binding::parse_str("i").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "disable",
                detail: "toggle disable selected strip group",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            }
            .with_palette("Toggle Selected Group", theme::GLYPH_ACTION_TOGGLE_OFF),
            Self::StripDCancel => ActionMeta {
                id: Self::StripDCancel,
                context: KeyContext::StripD,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel strip delete",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::StripPendingD => ActionMeta {
                id: Self::StripPendingD,
                context: KeyContext::Strip,
                toml_key: "pending_d",
                default: Binding::parse_str("d").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "del…",
                detail: "dd delete / di disable",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::StripPrevGroup => ActionMeta {
                id: Self::StripPrevGroup,
                context: KeyContext::Strip,
                toml_key: "prev_group",
                default: Binding::parse_str("h").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "group",
                detail: "previous strip group",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::StripNextGroup => ActionMeta {
                id: Self::StripNextGroup,
                context: KeyContext::Strip,
                toml_key: "next_group",
                default: Binding::parse_str("l").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "group",
                detail: "next strip group",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::StripResumeFollow => ActionMeta {
                id: Self::StripResumeFollow,
                context: KeyContext::Strip,
                toml_key: "resume_follow",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "follow",
                detail: "resume following",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::StripOpenHelp => ActionMeta {
                id: Self::StripOpenHelp,
                context: KeyContext::Strip,
                toml_key: "open_help",
                default: Binding::parse_str("?").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "help",
                detail: "open help",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::StripFocusNext => ActionMeta {
                id: Self::StripFocusNext,
                context: KeyContext::Strip,
                toml_key: "focus_next",
                default: Binding::parse_str("Tab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "focus",
                detail: "cycle focus",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::VisualMoveDown => ActionMeta {
                id: Self::VisualMoveDown,
                context: KeyContext::Visual,
                toml_key: "move_down",
                default: Binding::parse_str("j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "move",
                detail: "extend selection down",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::VisualMoveUp => ActionMeta {
                id: Self::VisualMoveUp,
                context: KeyContext::Visual,
                toml_key: "move_up",
                default: Binding::parse_str("k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "move",
                detail: "extend selection up",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::VisualJumpDown => ActionMeta {
                id: Self::VisualJumpDown,
                context: KeyContext::Visual,
                toml_key: "jump_down",
                default: Binding::parse_str("S-j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "jump",
                detail: "extend selection down fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::VisualJumpUp => ActionMeta {
                id: Self::VisualJumpUp,
                context: KeyContext::Visual,
                toml_key: "jump_up",
                default: Binding::parse_str("S-k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "jump",
                detail: "extend selection up fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::VisualYankRaw => ActionMeta {
                id: Self::VisualYankRaw,
                context: KeyContext::Visual,
                toml_key: "yank_raw",
                default: Binding::parse_str("y").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "yank",
                detail: "yank selection raw",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::VisualYankMsg => ActionMeta {
                id: Self::VisualYankMsg,
                context: KeyContext::Visual,
                toml_key: "yank_msg",
                default: Binding::parse_str("S-y").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "yank",
                detail: "yank selection messages",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::VisualCancel => ActionMeta {
                id: Self::VisualCancel,
                context: KeyContext::Visual,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "leave visual mode",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpClose => ActionMeta {
                id: Self::HelpClose,
                context: KeyContext::Help,
                toml_key: "close",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "close",
                detail: "close help",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpToggle => ActionMeta {
                id: Self::HelpToggle,
                context: KeyContext::Help,
                toml_key: "toggle",
                default: Binding::parse_str("?").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "close",
                detail: "close help",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpScrollDown => ActionMeta {
                id: Self::HelpScrollDown,
                context: KeyContext::Help,
                toml_key: "scroll_down",
                default: Binding::parse_str("j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "scroll",
                detail: "scroll help down",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpScrollUp => ActionMeta {
                id: Self::HelpScrollUp,
                context: KeyContext::Help,
                toml_key: "scroll_up",
                default: Binding::parse_str("k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "scroll",
                detail: "scroll help up",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpJumpDown => ActionMeta {
                id: Self::HelpJumpDown,
                context: KeyContext::Help,
                toml_key: "jump_down",
                default: Binding::parse_str("S-j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "jump",
                detail: "scroll help down fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpJumpUp => ActionMeta {
                id: Self::HelpJumpUp,
                context: KeyContext::Help,
                toml_key: "jump_up",
                default: Binding::parse_str("S-k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "jump",
                detail: "scroll help up fast",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpTop => ActionMeta {
                id: Self::HelpTop,
                context: KeyContext::Help,
                toml_key: "top",
                default: Binding::parse_str("g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "top",
                detail: "scroll help to top",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpBottom => ActionMeta {
                id: Self::HelpBottom,
                context: KeyContext::Help,
                toml_key: "bottom",
                default: Binding::parse_str("S-g").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "bottom",
                detail: "scroll help to bottom",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpBack => ActionMeta {
                id: Self::HelpBack,
                context: KeyContext::Help,
                toml_key: "back",
                default: Binding::parse_str("h").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "back",
                detail: "return to help home",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpBackAlt => ActionMeta {
                id: Self::HelpBackAlt,
                context: KeyContext::Help,
                toml_key: "back_alt",
                default: Binding::parse_str("Backspace").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "back",
                detail: "return to help home",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpSearch => ActionMeta {
                id: Self::HelpSearch,
                context: KeyContext::Help,
                toml_key: "search",
                default: Binding::parse_str("/").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "search",
                detail: "search help",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpSearchNext => ActionMeta {
                id: Self::HelpSearchNext,
                context: KeyContext::Help,
                toml_key: "search_next",
                default: Binding::parse_str("n").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "next",
                detail: "next help search hit",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpSearchPrev => ActionMeta {
                id: Self::HelpSearchPrev,
                context: KeyContext::Help,
                toml_key: "search_prev",
                default: Binding::parse_str("S-n").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "prev",
                detail: "previous help search hit",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HelpSubmit => ActionMeta {
                id: Self::HelpSubmit,
                context: KeyContext::Help,
                toml_key: "submit",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "open",
                detail: "open help page or commit search",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerSubmit => ActionMeta {
                id: Self::PickerSubmit,
                context: KeyContext::Picker,
                toml_key: "submit",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "toggle",
                detail: "enable/disable or submit",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerUp => ActionMeta {
                id: Self::PickerUp,
                context: KeyContext::Picker,
                toml_key: "up",
                default: Binding::parse_str("Up").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "select",
                detail: "previous candidate",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerDown => ActionMeta {
                id: Self::PickerDown,
                context: KeyContext::Picker,
                toml_key: "down",
                default: Binding::parse_str("Down").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "select",
                detail: "next candidate",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerMulti => ActionMeta {
                id: Self::PickerMulti,
                context: KeyContext::Picker,
                toml_key: "multi",
                default: Binding::parse_str("Tab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "multi",
                detail: "toggle multi-select",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerEdit => ActionMeta {
                id: Self::PickerEdit,
                context: KeyContext::Picker,
                toml_key: "edit",
                default: Binding::parse_str("C-x").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "edit",
                detail: "edit selected",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerDelete => ActionMeta {
                id: Self::PickerDelete,
                context: KeyContext::Picker,
                toml_key: "delete",
                default: Binding::parse_str("Delete").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "delete",
                detail: "delete with confirm",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerDeleteAlt => ActionMeta {
                id: Self::PickerDeleteAlt,
                context: KeyContext::Picker,
                toml_key: "delete_alt",
                default: Binding::parse_str("C-Backspace").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "delete",
                detail: "delete with confirm",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PickerClose => ActionMeta {
                id: Self::PickerClose,
                context: KeyContext::Picker,
                toml_key: "close",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "close",
                detail: "close picker",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ConfirmYes => ActionMeta {
                id: Self::ConfirmYes,
                context: KeyContext::Confirm,
                toml_key: "yes",
                default: Binding::parse_str("y").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "confirm",
                detail: "confirm",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ConfirmYesEnter => ActionMeta {
                id: Self::ConfirmYesEnter,
                context: KeyContext::Confirm,
                toml_key: "yes_enter",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "confirm",
                detail: "confirm",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ConfirmNo => ActionMeta {
                id: Self::ConfirmNo,
                context: KeyContext::Confirm,
                toml_key: "no",
                default: Binding::parse_str("n").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::ConfirmCancel => ActionMeta {
                id: Self::ConfirmCancel,
                context: KeyContext::Confirm,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::DetailCloseFields => ActionMeta {
                id: Self::DetailCloseFields,
                context: KeyContext::Detail,
                toml_key: "close_fields",
                default: Binding::parse_str("p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "close",
                detail: "close detail",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::DetailSwap => ActionMeta {
                id: Self::DetailSwap,
                context: KeyContext::Detail,
                toml_key: "swap",
                default: Binding::parse_str("S-p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "swap",
                detail: "swap fields/pretty",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::DetailChip => ActionMeta {
                id: Self::DetailChip,
                context: KeyContext::Detail,
                toml_key: "chip",
                default: Binding::parse_str("c").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "chip",
                detail: "filter field from detail",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::DetailExclude => ActionMeta {
                id: Self::DetailExclude,
                context: KeyContext::Detail,
                toml_key: "exclude",
                default: Binding::parse_str("S-c").expect("default binding"),
                kind: ActionKind::Prefix,
                capabilities: &[],
                label: "exclude",
                detail: "exclude field from detail",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::DetailMoveDown => ActionMeta {
                id: Self::DetailMoveDown,
                context: KeyContext::Detail,
                toml_key: "move_down",
                default: Binding::parse_str("j").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "row",
                detail: "next row",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::DetailMoveUp => ActionMeta {
                id: Self::DetailMoveUp,
                context: KeyContext::Detail,
                toml_key: "move_up",
                default: Binding::parse_str("k").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "row",
                detail: "previous row",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::DetailClose => ActionMeta {
                id: Self::DetailClose,
                context: KeyContext::Detail,
                toml_key: "close",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "close",
                detail: "close detail",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::TimePanelNext => ActionMeta {
                id: Self::TimePanelNext,
                context: KeyContext::TimePanel,
                toml_key: "next",
                default: Binding::parse_str("Tab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "next",
                detail: "next field",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::TimePanelSubmit => ActionMeta {
                id: Self::TimePanelSubmit,
                context: KeyContext::TimePanel,
                toml_key: "submit",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "next",
                detail: "next field / submit",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::TimePanelDateUp => ActionMeta {
                id: Self::TimePanelDateUp,
                context: KeyContext::TimePanel,
                toml_key: "date_up",
                default: Binding::parse_str("Up").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "date",
                detail: "previous date",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::TimePanelDateDown => ActionMeta {
                id: Self::TimePanelDateDown,
                context: KeyContext::TimePanel,
                toml_key: "date_down",
                default: Binding::parse_str("Down").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "date",
                detail: "next date",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::TimePanelCancel => ActionMeta {
                id: Self::TimePanelCancel,
                context: KeyContext::TimePanel,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel time panel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::InputDraftSpace => ActionMeta {
                id: Self::InputDraftSpace,
                context: KeyContext::Input,
                toml_key: "draft_space",
                default: Binding::parse_str("Space").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "draft",
                detail: "space in draft",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::InputCommit => ActionMeta {
                id: Self::InputCommit,
                context: KeyContext::Input,
                toml_key: "commit",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "commit",
                detail: "pill then submit group",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::InputToggleExclude => ActionMeta {
                id: Self::InputToggleExclude,
                context: KeyContext::Input,
                toml_key: "toggle_exclude",
                default: Binding::parse_str("!").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "exclude",
                detail: "toggle exclude draft",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::InputCancel => ActionMeta {
                id: Self::InputCancel,
                context: KeyContext::Input,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel input",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HighlightModalDraftSpace => ActionMeta {
                id: Self::HighlightModalDraftSpace,
                context: KeyContext::HighlightModal,
                toml_key: "draft_space",
                default: Binding::parse_str("Space").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "draft",
                detail: "space in draft",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HighlightModalConfirm => ActionMeta {
                id: Self::HighlightModalConfirm,
                context: KeyContext::HighlightModal,
                toml_key: "confirm",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "ok",
                detail: "confirm pattern",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HighlightModalConfirmTab => ActionMeta {
                id: Self::HighlightModalConfirmTab,
                context: KeyContext::HighlightModal,
                toml_key: "confirm_tab",
                default: Binding::parse_str("Tab").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "ok",
                detail: "confirm pattern",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::HighlightModalCancel => ActionMeta {
                id: Self::HighlightModalCancel,
                context: KeyContext::HighlightModal,
                toml_key: "cancel",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "cancel",
                detail: "cancel",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::GlobalCommandPalette => ActionMeta {
                id: Self::GlobalCommandPalette,
                context: KeyContext::Global,
                toml_key: "command_palette",
                default: Binding::parse_str("C-p").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "palette",
                detail: "open command palette",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PaletteSubmit => ActionMeta {
                id: Self::PaletteSubmit,
                context: KeyContext::CommandPalette,
                toml_key: "submit",
                default: Binding::parse_str("Enter").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "run",
                detail: "run selected command",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PaletteUp => ActionMeta {
                id: Self::PaletteUp,
                context: KeyContext::CommandPalette,
                toml_key: "up",
                default: Binding::parse_str("Up").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "up",
                detail: "select previous command",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PaletteDown => ActionMeta {
                id: Self::PaletteDown,
                context: KeyContext::CommandPalette,
                toml_key: "down",
                default: Binding::parse_str("Down").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "down",
                detail: "select next command",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
            Self::PaletteClose => ActionMeta {
                id: Self::PaletteClose,
                context: KeyContext::CommandPalette,
                toml_key: "close",
                default: Binding::parse_str("Esc").expect("default binding"),
                kind: ActionKind::Leaf,
                capabilities: &[],
                label: "close",
                detail: "close command palette",
                in_palette: false,
                palette_title: "",
                palette_icon: "",
            },
        }
    }

    pub fn context(self) -> KeyContext {
        self.meta().context
    }
    pub fn toml_key(self) -> &'static str {
        self.meta().toml_key
    }
}

fn action_by_toml(ctx: KeyContext, key: &str) -> Option<ActionId> {
    match (ctx, key) {
        (KeyContext::Global, "quit") => Some(ActionId::GlobalQuit),
        (KeyContext::Global, "focus_next") => Some(ActionId::GlobalFocusNext),
        (KeyContext::Global, "focus_prev") => Some(ActionId::GlobalFocusPrev),
        (KeyContext::Global, "focus_filter") => Some(ActionId::GlobalFocusFilter),
        (KeyContext::Global, "focus_exclude") => Some(ActionId::GlobalFocusExclude),
        (KeyContext::Global, "focus_highlight") => Some(ActionId::GlobalFocusHighlight),
        (KeyContext::Global, "focus_log") => Some(ActionId::GlobalFocusLog),
        (KeyContext::Global, "focus_input") => Some(ActionId::GlobalFocusInput),
        (KeyContext::Global, "filter_new") => Some(ActionId::GlobalFilterNew),
        (KeyContext::Global, "highlight_new") => Some(ActionId::GlobalHighlightNew),
        (KeyContext::Global, "highlight_add") => Some(ActionId::GlobalHighlightAdd),
        (KeyContext::Global, "exclude_new") => Some(ActionId::GlobalExcludeNew),
        (KeyContext::Global, "open_help") => Some(ActionId::GlobalOpenHelp),
        (KeyContext::Global, "command_palette") => Some(ActionId::GlobalCommandPalette),
        (KeyContext::Global, "preset_save") => Some(ActionId::LeaderPresetSave),
        (KeyContext::Global, "preset_open") => Some(ActionId::LeaderPresetOpen),
        (KeyContext::Global, "open_file") => Some(ActionId::OpenFile),
        (KeyContext::Global, "open_stream") => Some(ActionId::OpenStream),
        (KeyContext::LogList, "move_down") => Some(ActionId::LogListMoveDown),
        (KeyContext::LogList, "move_up") => Some(ActionId::LogListMoveUp),
        (KeyContext::LogList, "jump_down") => Some(ActionId::LogListJumpDown),
        (KeyContext::LogList, "jump_up") => Some(ActionId::LogListJumpUp),
        (KeyContext::LogList, "jump_top") => Some(ActionId::LogListJumpTop),
        (KeyContext::LogList, "jump_bottom") => Some(ActionId::LogListJumpBottom),
        (KeyContext::LogList, "resume_follow") => Some(ActionId::LogListResumeFollow),
        (KeyContext::LogList, "next_match") => Some(ActionId::LogListNextMatch),
        (KeyContext::LogList, "prev_match") => Some(ActionId::LogListPrevMatch),
        (KeyContext::LogList, "next_severe") => Some(ActionId::LogListNextSevere),
        (KeyContext::LogList, "prev_severe") => Some(ActionId::LogListPrevSevere),
        (KeyContext::LogList, "detail_fields") => Some(ActionId::LogListDetailFields),
        (KeyContext::LogList, "detail_pretty") => Some(ActionId::LogListDetailPretty),
        (KeyContext::LogList, "visual_line") => Some(ActionId::LogListVisualLine),
        (KeyContext::LogList, "yank_msg_line") => Some(ActionId::LogListYankMsgLine),
        (KeyContext::LogList, "clear_live") => Some(ActionId::LogListClearLive),
        (KeyContext::LogList, "page_down") => Some(ActionId::LogListPageDown),
        (KeyContext::LogList, "page_up") => Some(ActionId::LogListPageUp),
        (KeyContext::LogList, "leader") => Some(ActionId::LogListLeader),
        (KeyContext::LogList, "bookmark") => Some(ActionId::LogListBookmark),
        (KeyContext::LogList, "chip") => Some(ActionId::LogListChip),
        (KeyContext::LogList, "exclude_chip") => Some(ActionId::LogListExcludeChip),
        (KeyContext::LogList, "yank") => Some(ActionId::LogListYank),
        (KeyContext::LogList, "lock") => Some(ActionId::LogListLock),
        (KeyContext::LogList, "time") => Some(ActionId::LogListTime),
        (KeyContext::LogList, "wrap_toggle") => Some(ActionId::LogListWrapToggle),
        (KeyContext::Leader, "manage") => Some(ActionId::LeaderManage),
        (KeyContext::Leader, "summary") => Some(ActionId::LeaderSummary),
        (KeyContext::Leader, "cancel") => Some(ActionId::LeaderCancel),
        (KeyContext::Bookmark, "add") => Some(ActionId::BookmarkAdd),
        (KeyContext::Bookmark, "remove") => Some(ActionId::BookmarkRemove),
        (KeyContext::Bookmark, "manage") => Some(ActionId::BookmarkManage),
        (KeyContext::Bookmark, "cancel") => Some(ActionId::BookmarkCancel),
        (KeyContext::Lock, "pid") => Some(ActionId::LockPid),
        (KeyContext::Lock, "tid") => Some(ActionId::LockTid),
        (KeyContext::Lock, "view_highlight") => Some(ActionId::LockViewHighlight),
        (KeyContext::Lock, "view_severe") => Some(ActionId::LockViewSevere),
        (KeyContext::Lock, "clear") => Some(ActionId::LockClear),
        (KeyContext::Lock, "cancel") => Some(ActionId::LockCancel),
        (KeyContext::Time, "set") => Some(ActionId::TimeSet),
        (KeyContext::Time, "clear") => Some(ActionId::TimeClear),
        (KeyContext::Time, "cancel") => Some(ActionId::TimeCancel),
        (KeyContext::Time, "histogram") => Some(ActionId::TimeHistogram),
        (KeyContext::HistPanel, "prev") => Some(ActionId::HistPanelPrev),
        (KeyContext::HistPanel, "next") => Some(ActionId::HistPanelNext),
        (KeyContext::HistPanel, "jump_down") => Some(ActionId::HistPanelJumpDown),
        (KeyContext::HistPanel, "jump_up") => Some(ActionId::HistPanelJumpUp),
        (KeyContext::HistPanel, "jump_top") => Some(ActionId::HistPanelJumpTop),
        (KeyContext::HistPanel, "jump_bottom") => Some(ActionId::HistPanelJumpBottom),
        (KeyContext::HistPanel, "zoom_in") => Some(ActionId::HistPanelZoomIn),
        (KeyContext::HistPanel, "zoom_out") => Some(ActionId::HistPanelZoomOut),
        (KeyContext::HistPanel, "submit") => Some(ActionId::HistPanelSubmit),
        (KeyContext::HistPanel, "apply_window") => Some(ActionId::HistPanelApplyWindow),
        (KeyContext::HistPanel, "cancel") => Some(ActionId::HistPanelCancel),
        (KeyContext::ChipField, "tag") => Some(ActionId::ChipFieldTag),
        (KeyContext::ChipField, "msg") => Some(ActionId::ChipFieldMsg),
        (KeyContext::ChipField, "pkg") => Some(ActionId::ChipFieldPkg),
        (KeyContext::ChipField, "pid") => Some(ActionId::ChipFieldPid),
        (KeyContext::ChipField, "tid") => Some(ActionId::ChipFieldTid),
        (KeyContext::ChipField, "level") => Some(ActionId::ChipFieldLevel),
        (KeyContext::ChipField, "cancel") => Some(ActionId::ChipFieldCancel),
        (KeyContext::Yank, "cli") => Some(ActionId::YankCli),
        (KeyContext::Yank, "tag") => Some(ActionId::YankTag),
        (KeyContext::Yank, "msg") => Some(ActionId::YankMsg),
        (KeyContext::Yank, "pkg") => Some(ActionId::YankPkg),
        (KeyContext::Yank, "pid") => Some(ActionId::YankPid),
        (KeyContext::Yank, "tid") => Some(ActionId::YankTid),
        (KeyContext::Yank, "level") => Some(ActionId::YankLevel),
        (KeyContext::Yank, "raw") => Some(ActionId::YankRaw),
        (KeyContext::Yank, "line") => Some(ActionId::YankLine),
        (KeyContext::Yank, "time") => Some(ActionId::YankTime),
        (KeyContext::Yank, "cancel") => Some(ActionId::YankCancel),
        (KeyContext::StripD, "delete") => Some(ActionId::StripDDelete),
        (KeyContext::StripD, "disable") => Some(ActionId::StripDDisable),
        (KeyContext::StripD, "cancel") => Some(ActionId::StripDCancel),
        (KeyContext::Strip, "pending_d") => Some(ActionId::StripPendingD),
        (KeyContext::Strip, "prev_group") => Some(ActionId::StripPrevGroup),
        (KeyContext::Strip, "next_group") => Some(ActionId::StripNextGroup),
        (KeyContext::Strip, "resume_follow") => Some(ActionId::StripResumeFollow),
        (KeyContext::Strip, "open_help") => Some(ActionId::StripOpenHelp),
        (KeyContext::Strip, "focus_next") => Some(ActionId::StripFocusNext),
        (KeyContext::Visual, "move_down") => Some(ActionId::VisualMoveDown),
        (KeyContext::Visual, "move_up") => Some(ActionId::VisualMoveUp),
        (KeyContext::Visual, "jump_down") => Some(ActionId::VisualJumpDown),
        (KeyContext::Visual, "jump_up") => Some(ActionId::VisualJumpUp),
        (KeyContext::Visual, "yank_raw") => Some(ActionId::VisualYankRaw),
        (KeyContext::Visual, "yank_msg") => Some(ActionId::VisualYankMsg),
        (KeyContext::Visual, "cancel") => Some(ActionId::VisualCancel),
        (KeyContext::Help, "close") => Some(ActionId::HelpClose),
        (KeyContext::Help, "toggle") => Some(ActionId::HelpToggle),
        (KeyContext::Help, "scroll_down") => Some(ActionId::HelpScrollDown),
        (KeyContext::Help, "scroll_up") => Some(ActionId::HelpScrollUp),
        (KeyContext::Help, "jump_down") => Some(ActionId::HelpJumpDown),
        (KeyContext::Help, "jump_up") => Some(ActionId::HelpJumpUp),
        (KeyContext::Help, "top") => Some(ActionId::HelpTop),
        (KeyContext::Help, "bottom") => Some(ActionId::HelpBottom),
        (KeyContext::Help, "back") => Some(ActionId::HelpBack),
        (KeyContext::Help, "back_alt") => Some(ActionId::HelpBackAlt),
        (KeyContext::Help, "search") => Some(ActionId::HelpSearch),
        (KeyContext::Help, "search_next") => Some(ActionId::HelpSearchNext),
        (KeyContext::Help, "search_prev") => Some(ActionId::HelpSearchPrev),
        (KeyContext::Help, "submit") => Some(ActionId::HelpSubmit),
        (KeyContext::Picker, "submit") => Some(ActionId::PickerSubmit),
        (KeyContext::Picker, "up") => Some(ActionId::PickerUp),
        (KeyContext::Picker, "down") => Some(ActionId::PickerDown),
        (KeyContext::Picker, "multi") => Some(ActionId::PickerMulti),
        (KeyContext::Picker, "edit") => Some(ActionId::PickerEdit),
        (KeyContext::Picker, "delete") => Some(ActionId::PickerDelete),
        (KeyContext::Picker, "delete_alt") => Some(ActionId::PickerDeleteAlt),
        (KeyContext::Picker, "close") => Some(ActionId::PickerClose),
        (KeyContext::Picker, "clear_all_rules") => Some(ActionId::ClearAllRules),
        (KeyContext::Confirm, "yes") => Some(ActionId::ConfirmYes),
        (KeyContext::Confirm, "yes_enter") => Some(ActionId::ConfirmYesEnter),
        (KeyContext::Confirm, "no") => Some(ActionId::ConfirmNo),
        (KeyContext::Confirm, "cancel") => Some(ActionId::ConfirmCancel),
        (KeyContext::Detail, "close_fields") => Some(ActionId::DetailCloseFields),
        (KeyContext::Detail, "swap") => Some(ActionId::DetailSwap),
        (KeyContext::Detail, "chip") => Some(ActionId::DetailChip),
        (KeyContext::Detail, "exclude") => Some(ActionId::DetailExclude),
        (KeyContext::Detail, "move_down") => Some(ActionId::DetailMoveDown),
        (KeyContext::Detail, "move_up") => Some(ActionId::DetailMoveUp),
        (KeyContext::Detail, "close") => Some(ActionId::DetailClose),
        (KeyContext::TimePanel, "next") => Some(ActionId::TimePanelNext),
        (KeyContext::TimePanel, "submit") => Some(ActionId::TimePanelSubmit),
        (KeyContext::TimePanel, "date_up") => Some(ActionId::TimePanelDateUp),
        (KeyContext::TimePanel, "date_down") => Some(ActionId::TimePanelDateDown),
        (KeyContext::TimePanel, "cancel") => Some(ActionId::TimePanelCancel),
        (KeyContext::Input, "draft_space") => Some(ActionId::InputDraftSpace),
        (KeyContext::Input, "commit") => Some(ActionId::InputCommit),
        (KeyContext::Input, "toggle_exclude") => Some(ActionId::InputToggleExclude),
        (KeyContext::Input, "cancel") => Some(ActionId::InputCancel),
        (KeyContext::HighlightModal, "draft_space") => Some(ActionId::HighlightModalDraftSpace),
        (KeyContext::HighlightModal, "confirm") => Some(ActionId::HighlightModalConfirm),
        (KeyContext::HighlightModal, "confirm_tab") => Some(ActionId::HighlightModalConfirmTab),
        (KeyContext::HighlightModal, "cancel") => Some(ActionId::HighlightModalCancel),
        (KeyContext::CommandPalette, "submit") => Some(ActionId::PaletteSubmit),
        (KeyContext::CommandPalette, "up") => Some(ActionId::PaletteUp),
        (KeyContext::CommandPalette, "down") => Some(ActionId::PaletteDown),
        (KeyContext::CommandPalette, "close") => Some(ActionId::PaletteClose),
        _ => None,
    }
}
/// Effective keymap after merge.
#[derive(Debug, Clone)]
pub struct KeymapStore {
    /// `None` means unbound.
    bindings: HashMap<ActionId, Option<Binding>>,
    pub warnings: Vec<String>,
}

impl Default for KeymapStore {
    fn default() -> Self {
        Self::builtin()
    }
}

impl KeymapStore {
    pub fn builtin() -> Self {
        let mut bindings = HashMap::with_capacity(ActionId::ALL.len());
        for &id in ActionId::ALL {
            let default = id.meta().default;
            bindings.insert(
                id,
                if default.strokes.is_empty() {
                    None
                } else {
                    Some(default)
                },
            );
        }
        Self {
            bindings,
            warnings: Vec::new(),
        }
    }

    pub fn binding(&self, id: ActionId) -> Option<&Binding> {
        self.bindings.get(&id).and_then(|b| b.as_ref())
    }

    pub fn display(&self, id: ActionId) -> Option<String> {
        self.binding(id).map(|b| {
            if b.strokes.len() == 1 {
                b.strokes[0].format_ui()
            } else {
                b.format_compact()
            }
        })
    }

    pub fn display_aggregate(&self, ids: &[ActionId]) -> Option<String> {
        let mut parts = Vec::new();
        for &id in ids {
            if let Some(s) = self.display(id) {
                parts.push(s);
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("/"))
        }
    }

    pub fn matches_stroke(&self, id: ActionId, stroke: &KeyStroke) -> bool {
        match self.binding(id) {
            Some(b) if b.strokes.len() == 1 => b.strokes[0] == *stroke,
            _ => false,
        }
    }

    pub fn matches_code(&self, id: ActionId, code: KeyCode) -> bool {
        let Some(stroke) = stroke_from_keycode(code) else {
            return false;
        };
        self.matches_stroke(id, &stroke)
    }

    pub fn matches_event(&self, id: ActionId, key: KeyEvent) -> bool {
        let Some(stroke) = stroke_from_event(key) else {
            return false;
        };
        self.matches_stroke(id, &stroke)
    }

    /// Find an action in `ctx` whose single-key binding equals `stroke`.
    pub fn action_for_stroke(
        &self,
        ctx: KeyContext,
        stroke: &KeyStroke,
        file_mode: bool,
    ) -> Option<ActionId> {
        for &id in ActionId::ALL {
            let meta = id.meta();
            if meta.context != ctx || !meta.allowed(file_mode) {
                continue;
            }
            if self.matches_stroke(id, stroke) {
                return Some(id);
            }
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapLoadStatus {
    Builtin,
    Loaded(PathBuf),
    Fallback { path: PathBuf, error: String },
}

impl KeymapLoadStatus {
    pub fn status_hint(&self) -> Option<String> {
        match self {
            Self::Fallback { error, .. } => Some(format!("KEYMAP fallback: {error}")),
            _ => None,
        }
    }

    pub fn warning_hint(warnings: &[String]) -> Option<String> {
        if warnings.is_empty() {
            None
        } else {
            Some(format!("KEYMAP warn: {}", warnings.join("; ")))
        }
    }
}

fn parse_bind_value(v: &toml::Value) -> Result<Option<Binding>, String> {
    match v {
        toml::Value::String(s) => {
            if s.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Binding::parse_str(s)?))
            }
        }
        toml::Value::Boolean(false) => Ok(None),
        toml::Value::Boolean(true) => Err("binding true is invalid; use a key string".into()),
        toml::Value::Array(arr) => {
            if arr.is_empty() {
                return Ok(None);
            }
            let mut strokes = Vec::with_capacity(arr.len());
            for a in arr {
                let s = a
                    .as_str()
                    .ok_or_else(|| "chord elements must be strings".to_string())?;
                strokes.push(KeyStroke::parse(s)?);
            }
            Ok(Some(Binding { strokes }))
        }
        other => Err(format!("invalid binding value: {other}")),
    }
}

fn validate_prefix_tree(store: &KeymapStore) -> Result<(), String> {
    let mut by_ctx: HashMap<KeyContext, Vec<(ActionId, Binding, ActionKind)>> = HashMap::new();
    for &id in ActionId::ALL {
        if let Some(b) = store.binding(id) {
            by_ctx
                .entry(id.context())
                .or_default()
                .push((id, b.clone(), id.meta().kind));
        }
    }
    for (ctx, list) in &by_ctx {
        for i in 0..list.len() {
            for j in 0..list.len() {
                if i == j {
                    continue;
                }
                let (id_a, ba, ka) = &list[i];
                let (id_b, bb, _) = &list[j];
                if ba.strokes == bb.strokes {
                    return Err(format!(
                        "duplicate binding {} for {:?} and {:?} in [{}]",
                        ba.format(),
                        id_a,
                        id_b,
                        ctx.as_toml()
                    ));
                }
                if ba.strokes.len() < bb.strokes.len()
                    && bb.strokes.starts_with(&ba.strokes)
                    && *ka == ActionKind::Leaf
                {
                    return Err(format!(
                        "leaf {:?} in [{}] is a prefix of {:?}",
                        id_a,
                        ctx.as_toml(),
                        id_b
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Merge user TOML onto builtins.
pub fn merge_user_toml(text: &str) -> Result<KeymapStore, String> {
    let table: BTreeMap<String, BTreeMap<String, toml::Value>> =
        toml::from_str(text).map_err(|e| e.to_string())?;
    let mut store = KeymapStore::builtin();
    let mut warnings = Vec::new();

    for (section, entries) in table {
        let Some(ctx) = KeyContext::from_toml(&section) else {
            warnings.push(format!("unknown section [{section}]"));
            continue;
        };
        for (key, val) in entries {
            let Some(id) = action_by_toml(ctx, &key) else {
                warnings.push(format!("unknown action {section}.{key}"));
                continue;
            };
            let parsed = parse_bind_value(&val).map_err(|e| format!("{section}.{key}: {e}"))?;
            store.bindings.insert(id, parsed);
        }
    }

    validate_prefix_tree(&store)?;
    store.warnings = warnings;
    Ok(store)
}

pub fn load_keymap(config_dir: &Path) -> (KeymapStore, KeymapLoadStatus) {
    let path = config_dir.join("keymap.toml");
    if !path.is_file() {
        return (KeymapStore::builtin(), KeymapLoadStatus::Builtin);
    }
    match fs::read_to_string(&path) {
        Ok(text) => match merge_user_toml(&text) {
            Ok(store) => (store, KeymapLoadStatus::Loaded(path)),
            Err(error) => (
                KeymapStore::builtin(),
                KeymapLoadStatus::Fallback { path, error },
            ),
        },
        Err(e) => (
            KeymapStore::builtin(),
            KeymapLoadStatus::Fallback {
                path,
                error: e.to_string(),
            },
        ),
    }
}

/// Serialize builtin defaults to an English-commented keymap.toml body.
pub fn serialize_default_toml() -> String {
    let mut out = String::from(
        "# alnav keymap.toml — override TUI keybindings.\n\
         # Deep-merged onto builtins: only list actions you want to change.\n\
         # Single key: string, e.g. move_down = \"j\"\n\
         # Chord: string array, e.g. some_chord = [\"m\", \"a\"]\n\
         # Modifiers: C- (Ctrl), S- (Shift), M- (Alt). Use S-j not J.\n\
         # Unbind: action = \"\" or action = null\n\n",
    );
    let mut items: Vec<(KeyContext, ActionId)> =
        ActionId::ALL.iter().map(|&id| (id.context(), id)).collect();
    items.sort_by_key(|(c, id)| (c.as_toml().to_string(), id.toml_key()));
    let mut current: Option<KeyContext> = None;
    for (ctx, id) in items {
        if current != Some(ctx) {
            if current.is_some() {
                out.push('\n');
            }
            out.push_str(&format!("[{}]\n", ctx.as_toml()));
            current = Some(ctx);
        }
        let meta = id.meta();
        out.push_str(&format!("# {} — {}\n", meta.label, meta.detail));
        let kind = match meta.kind {
            ActionKind::Prefix => "prefix",
            ActionKind::Leaf => "leaf",
        };
        out.push_str(&format!("# kind: {kind}\n"));
        out.push_str(&format!(
            "{} = \"{}\"\n",
            meta.toml_key,
            meta.default.format()
        ));
    }
    out
}

/// English-commented default `config.toml` body for `--init`.
pub fn serialize_default_config_toml() -> String {
    "# alnav config.toml — application settings.\n\
     #\n\
     # TUI theme (restart to apply). Builtins and signature accent:\n\
     #   default            cyan; no canvas paint; solid wordmark\n\
     #   onedark            blue\n\
     #   dracula            magenta\n\
     #   everforest         green\n\
     #   tokyo-night        blue      (aliases: TokyoNight, tokyo_night)\n\
     #   catppuccin-mocha   magenta   (aliases: catppuccin, mocha)\n\
     #   gruvbox-dark       yellow    (alias: gruvbox)\n\
     #   nord               cyan\n\
     #   kanagawa           blue      (alias: kanagawa-wave)\n\
     # Optional color overlay: theme.toml in this directory.\n\
     theme = \"default\"\n\
     #\n\
     # picker_left_ratio: width fraction of the left (candidate) pane in pickers.\n\
     # Right pane = 1 - this value. Clamped to [0.2, 0.8]. Default 0.4.\n\
     picker_left_ratio = 0.4\n\
     #\n\
     # picker_preview_enabled: show the right Preview column in pickers.\n\
     # When false, pickers are full-width. The unified manage panel (Space Space)\n\
     # is always full-width regardless of this setting.\n\
     picker_preview_enabled = true\n\
     #\n\
     # recent_files_limit: max paths remembered for Dashboard / C-f (1..=200).\n\
     recent_files_limit = 20\n\
     #\n\
     # log_dirs: directories recursively scanned for Open-file (C-f) fuzzy corpus.\n\
     # Empty = recent-only. Supports ~ expansion. No cwd fallback.\n\
     log_dirs = []\n\
     #\n\
     # log_extensions: case-insensitive suffix filter for corpus files.\n\
     # Empty / omitted → default [\".log\", \".txt\"].\n\
     log_extensions = [\".log\", \".txt\"]\n"
        .to_string()
}

/// Write default config/keymap templates. Returns human-readable status lines.
pub fn init_config_dir(config_dir: &Path, force: bool) -> Result<Vec<String>, String> {
    fs::create_dir_all(config_dir).map_err(|e| format!("create config dir: {e}"))?;
    let mut messages = Vec::new();
    for (name, body) in [
        ("config.toml", serialize_default_config_toml()),
        ("keymap.toml", serialize_default_toml()),
    ] {
        let path = config_dir.join(name);
        if path.exists() && !force {
            messages.push(format!("skip {} (exists)", path.display()));
            continue;
        }
        fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
        let verb = if force && path.exists() {
            "wrote"
        } else {
            "created"
        };
        // path.exists was true before write when force; after write always exists.
        let _ = verb;
        messages.push(format!("wrote {}", path.display()));
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stroke_modifiers_and_reject_bare_upper() {
        let s = KeyStroke::parse("C-s").unwrap();
        assert!(s.ctrl);
        assert_eq!(s.code, StrokeCode::Char('s'));
        let s = KeyStroke::parse("S-j").unwrap();
        assert!(s.shift);
        assert!(KeyStroke::parse("J").is_err());
        assert_eq!(
            KeyStroke::parse("Space").unwrap().code,
            StrokeCode::Char(' ')
        );
    }

    #[test]
    fn stroke_from_keycode_shift_letter() {
        let s = stroke_from_keycode(KeyCode::Char('J')).unwrap();
        assert!(s.shift);
        assert_eq!(s.code, StrokeCode::Char('j'));
    }

    #[test]
    fn builtin_defaults_roundtrip_critical() {
        let store = KeymapStore::builtin();
        assert!(store.matches_code(ActionId::LogListMoveDown, KeyCode::Char('j')));
        assert!(store.matches_code(ActionId::LogListJumpDown, KeyCode::Char('J')));
        assert!(store.matches_code(ActionId::LogListLeader, KeyCode::Char(' ')));
        assert!(store.matches_code(ActionId::LogListExcludeChip, KeyCode::Char('C')));
        assert!(store.matches_code(ActionId::LeaderManage, KeyCode::Char(' ')));
        assert!(store.matches_code(ActionId::BookmarkAdd, KeyCode::Char('a')));
        assert!(store.matches_event(
            ActionId::LeaderPresetSave,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        ));
        assert!(store.matches_event(
            ActionId::LeaderPresetOpen,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        ));
        assert!(store.matches_event(
            ActionId::OpenFile,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        ));
        assert!(store.matches_event(
            ActionId::OpenStream,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
        ));
        assert!(!store.matches_event(
            ActionId::OpenFile,
            KeyEvent::new(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
        ));
        assert!(!store.matches_code(ActionId::OpenFile, KeyCode::Char('f')));
        assert!(!store.matches_code(ActionId::OpenStream, KeyCode::Char('s')));
        assert!(!store.matches_code(ActionId::LeaderPresetSave, KeyCode::Char('w')));
        assert!(!store.matches_code(ActionId::LeaderPresetOpen, KeyCode::Char('o')));
        assert_eq!(
            store.display(ActionId::LogListClearLive).as_deref(),
            Some("C-l")
        );
        assert!(store.matches_event(
            ActionId::GlobalCommandPalette,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        ));
        assert!(store.matches_code(ActionId::HistPanelJumpDown, KeyCode::Char('J')));
        assert!(store.matches_code(ActionId::HistPanelJumpUp, KeyCode::Char('K')));
        assert!(store.matches_code(ActionId::HistPanelJumpTop, KeyCode::Char('g')));
        assert!(store.matches_code(ActionId::HistPanelJumpBottom, KeyCode::Char('G')));
        assert!(store.matches_code(ActionId::HistPanelZoomOut, KeyCode::Tab));
        assert!(store.matches_code(ActionId::HistPanelZoomIn, KeyCode::BackTab));
        assert!(!store.matches_code(ActionId::HistPanelZoomOut, KeyCode::Char('z')));
        assert!(!store.matches_code(ActionId::HistPanelZoomIn, KeyCode::Char('Z')));
        assert!(!store.matches_code(ActionId::GlobalCommandPalette, KeyCode::Char('p')));
        assert!(!store.matches_code(ActionId::GlobalCommandPalette, KeyCode::Char(':')));
        assert!(store.matches_code(ActionId::PaletteSubmit, KeyCode::Enter));
        assert!(store.matches_code(ActionId::PaletteClose, KeyCode::Esc));
    }

    #[test]
    fn merge_override_and_unbind() {
        let store = merge_user_toml(
            r#"
[log_list]
move_down = "Down"
move_up = ""
"#,
        )
        .unwrap();
        assert!(store.matches_code(ActionId::LogListMoveDown, KeyCode::Down));
        assert!(!store.matches_code(ActionId::LogListMoveDown, KeyCode::Char('j')));
        assert!(store.binding(ActionId::LogListMoveUp).is_none());
        // untouched default
        assert!(store.matches_code(ActionId::LogListJumpTop, KeyCode::Char('g')));
    }

    #[test]
    fn unknown_action_warns_but_applies_rest() {
        let store = merge_user_toml(
            r#"
[log_list]
move_down = "Down"
no_such = "x"
"#,
        )
        .unwrap();
        assert!(store.matches_code(ActionId::LogListMoveDown, KeyCode::Down));
        assert!(store.warnings.iter().any(|w| w.contains("no_such")));
    }

    #[test]
    fn duplicate_binding_fails_hard() {
        let err = merge_user_toml(
            r#"
[log_list]
move_down = "k"
"#,
        )
        .unwrap_err();
        assert!(err.contains("duplicate") || err.contains("prefix"), "{err}");
    }

    #[test]
    fn serialize_contains_sections() {
        let text = serialize_default_toml();
        assert!(text.contains("[log_list]"));
        assert!(text.contains("move_down = \"j\""));
        assert!(text.contains("[leader]"));
        assert!(text.contains("preset_save = \"C-s\""));
        assert!(text.contains("preset_open = \"C-o\""));
        assert!(text.contains("open_file = \"C-f\""));
        assert!(text.contains("open_stream = \"C-g\""));
        assert!(!text.contains("[open]"));
        assert!(!text.contains("open = \"o\""));
        assert!(text.contains("[command_palette]"));
        assert!(text.contains("command_palette = \"C-p\""));
        assert!(text.contains("highlight_new = \"/\""));
        assert!(text.contains("highlight_add = \"\""));
        assert!(text.contains("clear_all_rules = \"C-k\""));
        assert!(text.contains("submit = \"Enter\""));
        assert!(text.contains("[help]"));
        assert!(text.contains("back = \"h\""));
        assert!(text.contains("back_alt = \"Backspace\""));
        assert!(text.contains("search = \"/\""));
        assert!(text.contains("search_next = \"n\""));
        assert!(text.contains("search_prev = \"S-n\""));
        assert!(text.contains("submit = \"Enter\""));
    }

    #[test]
    fn default_config_toml_documents_theme_names() {
        let text = serialize_default_config_toml();
        assert!(text.contains("theme = \"default\""));
        for name in [
            "onedark",
            "dracula",
            "everforest",
            "tokyo-night",
            "catppuccin-mocha",
            "gruvbox-dark",
            "nord",
            "kanagawa",
        ] {
            assert!(text.contains(name), "missing {name}");
        }
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(parsed["theme"].as_str(), Some("default"));
        assert!(text.contains("C-f"), "Open-file comments must use C-f");
        assert!(!text.contains("C-S-o"), "must not document Ctrl-Shift open");
        assert!(
            !text.contains(" / of"),
            "must not document retired of chord"
        );
    }

    #[test]
    fn init_writes_and_skips() {
        let dir = tempfile_dir();
        let msgs = init_config_dir(&dir, false).unwrap();
        assert!(msgs.iter().any(|m| m.contains("keymap.toml")));
        assert!(dir.join("config.toml").is_file());
        assert!(dir.join("keymap.toml").is_file());
        let keymap = fs::read_to_string(dir.join("keymap.toml")).unwrap();
        assert!(keymap.contains("[command_palette]"));
        assert!(keymap.contains("command_palette = \"C-p\""));
        assert!(keymap.contains("[help]"));
        assert!(keymap.contains("search = \"/\""));
        assert!(keymap.contains("back = \"h\""));
        assert!(!dir.join("theme.toml").exists());
        let msgs2 = init_config_dir(&dir, false).unwrap();
        assert!(msgs2.iter().all(|m| m.starts_with("skip")));
    }

    fn tempfile_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("alnav-keymap-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn file_only_capability() {
        let meta = ActionId::LogListTime.meta();
        assert!(meta.allowed(true));
        assert!(!meta.allowed(false));
    }
}
