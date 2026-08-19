//! Action authority: palette catalog, `when` predicates, and `dispatch`.
//!
//! [`ActionId`] / [`ActionMeta`] / bindings live in [`crate::keymap`]. This
//! module owns what an action *does* and which intent commands appear in the
//! command palette.

use crate::app::{App, Focus, LockKind, StripKind, ViewFocusKind, YankField};
use crate::help::FAST_SCROLL_STEP;
use crate::input::ChipField;
use crate::keymap::ActionId;
use crate::picker::PickerKind;

/// LogList Ctrl-d / Ctrl-u page size (same value historically in `main.rs`).
pub const PAGE_SIZE: isize = 10;

/// One command-palette row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub id: ActionId,
    pub title: &'static str,
    pub icon: &'static str,
    pub key_hint: String,
}

/// Catalog ids in design.md order (`in_palette = true`).
pub const PALETTE_IDS: &[ActionId] = &[
    ActionId::GlobalFilterNew,
    ActionId::GlobalHighlightNew,
    ActionId::GlobalHighlightAdd,
    ActionId::GlobalExcludeNew,
    ActionId::GlobalOpenHelp,
    ActionId::GlobalQuit,
    ActionId::LogListWrapToggle,
    ActionId::LogListDetailFields,
    ActionId::LogListDetailPretty,
    ActionId::LogListClearLive,
    ActionId::LogListResumeFollow,
    ActionId::LeaderManage,
    ActionId::LeaderPresetSave,
    ActionId::LeaderPresetOpen,
    ActionId::LeaderSummary,
    ActionId::OpenFile,
    ActionId::OpenStream,
    ActionId::TimeSet,
    ActionId::TimeClear,
    ActionId::LockPid,
    ActionId::LockTid,
    ActionId::LockViewHighlight,
    ActionId::LockViewSevere,
    ActionId::LockClear,
    ActionId::BookmarkAdd,
    ActionId::BookmarkRemove,
    ActionId::BookmarkManage,
    ActionId::YankCli,
    ActionId::LogListYankMsgLine,
    ActionId::StripDDelete,
    ActionId::StripDDisable,
];

/// Extra `when` beyond [`crate::keymap::ActionMeta::allowed`].
pub fn when(app: &App, id: ActionId) -> bool {
    let file_mode = app.is_file_mode();
    if !id.meta().allowed(file_mode) {
        return false;
    }
    match id {
        ActionId::LogListDetailFields
        | ActionId::LogListDetailPretty
        | ActionId::BookmarkAdd
        | ActionId::LogListYankMsgLine => app.current_row().is_some(),
        ActionId::LockPid => app.current_row().is_some_and(|row| !row.pid.is_empty()),
        ActionId::LockTid => app.current_row().is_some_and(|row| !row.tid.is_empty()),
        ActionId::BookmarkRemove => app
            .current_row()
            .is_some_and(|row| app.is_bookmark_row(row.row_id)),
        ActionId::TimeSet => file_mode && app.has_time_date_candidates(),
        ActionId::TimeClear => file_mode && app.time_bound.is_some(),
        ActionId::LogListClearLive => true, // LiveOnly on meta
        ActionId::LogListResumeFollow => !app.following,
        ActionId::LockClear => app.lock_pid.is_some() || app.lock_tid.is_some(),
        ActionId::StripDDelete | ActionId::StripDDisable => app.focused_strip_has_selection(),
        _ => true,
    }
}

/// Visible palette commands for `app` (capability + `when`; omitted, not dimmed).
pub fn catalog(app: &App) -> Vec<PaletteItem> {
    PALETTE_IDS
        .iter()
        .copied()
        .filter(|&id| when(app, id))
        .filter_map(|id| palette_item(app, id))
        .collect()
}

fn palette_item(app: &App, id: ActionId) -> Option<PaletteItem> {
    let meta = id.meta();
    if !meta.in_palette {
        return None;
    }
    Some(PaletteItem {
        id,
        title: meta.palette_title,
        icon: meta.palette_icon,
        key_hint: app.keymap.display(id).unwrap_or_default(),
    })
}

/// Fuzzy-filter [`catalog`] by `palette_title`. Empty query → no rows.
pub fn filtered_catalog(app: &App, query: &str) -> Vec<PaletteItem> {
    if query.is_empty() {
        return Vec::new();
    }
    let items = catalog(app);
    let titles: Vec<String> = items.iter().map(|i| i.title.to_string()).collect();
    crate::fuzzy::fuzzy_label_indices(&titles, query)
        .into_iter()
        .map(|i| items[i].clone())
        .collect()
}

/// Run the side effect for `id`. Prefix actions arm `pending_*`.
pub fn dispatch(app: &mut App, id: ActionId) {
    use ActionId::*;
    match id {
        GlobalQuit => app.should_quit = true,
        GlobalFocusNext | StripFocusNext => app.cycle_visible_focus_forward(),
        GlobalFocusPrev => app.cycle_visible_focus_backward(),
        GlobalFocusFilter => app.focus = Focus::ChipStrip,
        GlobalFocusExclude => app.focus = Focus::ExcludeStrip,
        GlobalFocusHighlight => app.focus = Focus::HighlightStrip,
        GlobalFocusLog => app.focus = Focus::LogList,
        GlobalFocusInput | LeaderManage => app.open_unified_picker(),
        GlobalFilterNew => app.open_picker_new(PickerKind::Filter),
        GlobalHighlightNew => app.open_highlight_finder(),
        GlobalHighlightAdd => app.open_picker_new(PickerKind::Highlight),
        GlobalExcludeNew => app.open_picker_new(PickerKind::Exclude),
        GlobalOpenHelp | StripOpenHelp => {
            if matches!(
                app.focus,
                Focus::LogList | Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
            ) && crate::help::help_available(app)
            {
                app.open_help();
            }
        }
        GlobalCommandPalette => app.open_command_palette(),
        LogListMoveDown | VisualMoveDown => app.move_cursor_manual(1),
        LogListMoveUp | VisualMoveUp => app.move_cursor_manual(-1),
        LogListJumpDown | VisualJumpDown => app.move_cursor_manual(FAST_SCROLL_STEP),
        LogListJumpUp | VisualJumpUp => app.move_cursor_manual(-FAST_SCROLL_STEP),
        LogListJumpTop => {
            app.following = false;
            app.jump_top();
        }
        LogListJumpBottom => app.resume_following(),
        LogListResumeFollow | StripResumeFollow | DetailClose => apply_esc_resume(app),
        LogListNextMatch => {
            if matches!(app.find_match(1), crate::app::FindJumpResult::NoMore) {
                app.set_flash("NO MORE");
            }
        }
        LogListPrevMatch => {
            if matches!(app.find_match(-1), crate::app::FindJumpResult::NoMore) {
                app.set_flash("NO MORE");
            }
        }
        LogListNextSevere => match app.find_severe(1) {
            crate::app::FindJumpResult::None => app.set_flash("NO ERROR"),
            crate::app::FindJumpResult::NoMore => app.set_flash("NO MORE"),
            crate::app::FindJumpResult::Moved => {}
        },
        LogListPrevSevere => match app.find_severe(-1) {
            crate::app::FindJumpResult::None => app.set_flash("NO ERROR"),
            crate::app::FindJumpResult::NoMore => app.set_flash("NO MORE"),
            crate::app::FindJumpResult::Moved => {}
        },
        LogListDetailFields => app.toggle_detail_fields(),
        LogListDetailPretty => app.toggle_detail_pretty(),
        LogListVisualLine => app.enter_visual_line(),
        LogListYankMsgLine => {
            if let Some(text) = app.yank_field(YankField::Msg) {
                app.apply_yank(text);
            }
        }
        LogListClearLive => {
            if app.export_source.is_live() && !app.detail_open() {
                app.clear_buffered_logs();
            }
        }
        LogListPageDown => app.move_cursor_manual(PAGE_SIZE),
        LogListPageUp => app.move_cursor_manual(-PAGE_SIZE),
        LogListLeader => {
            app.clear_visual();
            app.clear_pending_all();
            app.pending_leader = true;
        }
        LogListBookmark => app.begin_bookmark_op(),
        LogListChip => app.begin_chip_from_cursor(),
        LogListExcludeChip => app.begin_exclude_from_cursor(),
        LogListYank => {
            app.clear_visual();
            app.clear_pending_all();
            app.pending_yank = true;
        }
        LogListLock => app.begin_lock_from_cursor(),
        LogListTime => {
            if app.is_file_mode() {
                app.begin_time_op();
            }
        }
        LogListWrapToggle => app.toggle_collapsed_view(),
        LeaderPresetSave => app.begin_preset_save(),
        LeaderPresetOpen => app.begin_preset_open(),
        LeaderSummary => app.open_summary_panel(),
        LeaderCancel | BookmarkCancel | LockCancel | TimeCancel | ChipFieldCancel | YankCancel
        | StripDCancel | VisualCancel => apply_cancel(app, id),
        BookmarkAdd => app.bookmark_add_current(),
        BookmarkRemove => app.bookmark_remove_current(),
        BookmarkManage => app.open_picker(PickerKind::Bookmark),
        LockPid => app.apply_session_lock(LockKind::Pid),
        LockTid => app.apply_session_lock(LockKind::Tid),
        LockViewHighlight => app.toggle_view_focus(ViewFocusKind::Highlight),
        LockViewSevere => app.toggle_view_focus(ViewFocusKind::Severe),
        LockClear => app.clear_session_lock(),
        OpenFile => app.open_file_source_panel(app.dashboard.is_some()),
        OpenStream => app.open_stream_source_panel(app.dashboard.is_some()),
        TimeSet => {
            if app.is_file_mode() {
                let _ = app.open_time_panel();
            }
        }
        TimeClear => app.clear_time_bound(),
        ChipFieldTag => apply_chip_field(app, ChipField::Tag),
        ChipFieldMsg => apply_chip_field(app, ChipField::Msg),
        ChipFieldPkg => apply_chip_field(app, ChipField::Pkg),
        ChipFieldPid => apply_chip_field(app, ChipField::Pid),
        ChipFieldTid => apply_chip_field(app, ChipField::Tid),
        ChipFieldLevel => apply_chip_field(app, ChipField::Level),
        YankCli => {
            app.pending_yank = false;
            let cmd = app.export_cli_command();
            app.apply_yank(cmd);
        }
        YankTag => yank_field(app, YankField::Tag),
        YankMsg => {
            app.pending_yank = false;
            app.begin_msg_token_picker(crate::picker::MsgChipPurpose::Yank);
        }
        YankPkg => yank_field(app, YankField::Pkg),
        YankPid => yank_field(app, YankField::Pid),
        YankTid => yank_field(app, YankField::Tid),
        YankLevel => yank_field(app, YankField::Level),
        YankRaw | YankLine => yank_field(app, YankField::Raw),
        YankTime => yank_field(app, YankField::Timestamp),
        StripDDelete => {
            if let Some(kind) = focused_strip(app) {
                app.delete_focused_strip_group(kind);
            }
            app.pending_d = false;
        }
        StripDDisable => {
            if let Some(kind) = focused_strip(app) {
                app.toggle_disable_focused(kind);
            }
            app.pending_d = false;
        }
        StripPendingD => {
            app.pending_leader = false;
            app.pending_d = true;
        }
        StripPrevGroup => {
            if let Some(kind) = focused_strip(app) {
                app.move_strip_cursor(kind, -1);
            }
        }
        StripNextGroup => {
            if let Some(kind) = focused_strip(app) {
                app.move_strip_cursor(kind, 1);
            }
        }
        VisualYankRaw => yank_visual(app, YankField::Raw),
        VisualYankMsg => yank_visual(app, YankField::Msg),
        PaletteSubmit | PaletteUp | PaletteDown | PaletteClose => {
            // Routed by `handle_palette_key`; not dispatched as Normal leaves.
        }
        // Modal-internal actions stay in their dedicated handlers.
        HelpClose
        | HelpToggle
        | HelpScrollDown
        | HelpScrollUp
        | HelpJumpDown
        | HelpJumpUp
        | HelpTop
        | HelpBottom
        | HelpBack
        | HelpBackAlt
        | HelpSearch
        | HelpSearchNext
        | HelpSearchPrev
        | HelpSubmit
        | PickerSubmit
        | PickerUp
        | PickerDown
        | PickerMulti
        | PickerEdit
        | PickerDelete
        | PickerDeleteAlt
        | PickerClose
        | ConfirmYes
        | ConfirmYesEnter
        | ConfirmNo
        | ConfirmCancel
        | DetailCloseFields
        | DetailSwap
        | DetailChip
        | DetailExclude
        | DetailMoveDown
        | DetailMoveUp
        | TimePanelNext
        | TimePanelSubmit
        | TimePanelDateUp
        | TimePanelDateDown
        | TimePanelCancel
        | InputDraftSpace
        | InputCommit
        | InputToggleExclude
        | InputCancel
        | HighlightModalDraftSpace
        | HighlightModalConfirm
        | HighlightModalConfirmTab
        | HighlightModalCancel => {}
    }
}

fn apply_esc_resume(app: &mut App) {
    if app.detail_open() {
        app.close_detail();
        app.focus = Focus::LogList;
    } else if app.focus == Focus::LogList {
        app.focus = Focus::LogList;
        app.resume_following();
    } else {
        app.focus = Focus::LogList;
    }
}

fn apply_cancel(app: &mut App, id: ActionId) {
    match id {
        ActionId::LeaderCancel => app.pending_leader = false,
        ActionId::BookmarkCancel => app.cancel_bookmark_op(),
        ActionId::LockCancel => app.cancel_lock_pending(),
        ActionId::TimeCancel => app.cancel_time_pending(),
        ActionId::ChipFieldCancel => app.cancel_chip_from_cursor(),
        ActionId::YankCancel => app.pending_yank = false,
        ActionId::StripDCancel => app.pending_d = false,
        ActionId::VisualCancel => {
            app.clear_visual();
            app.focus = Focus::LogList;
            app.resume_following();
        }
        _ => {}
    }
}

fn apply_chip_field(app: &mut App, field: ChipField) {
    let exclude = app.pending_exclude;
    app.pending_chip = false;
    app.pending_exclude = false;
    match field {
        ChipField::Msg => {
            app.begin_msg_token_picker(crate::picker::MsgChipPurpose::Chip { exclude });
        }
        other => {
            if exclude {
                let _ = app.push_exclude_from_field(other);
            } else {
                let _ = app.push_chip_from_field(other);
            }
        }
    }
}

fn yank_field(app: &mut App, field: YankField) {
    app.pending_yank = false;
    if let Some(text) = app.yank_field(field) {
        app.apply_yank(text);
    }
}

fn yank_visual(app: &mut App, field: YankField) {
    if let Some((lo, hi)) = app.selection_range() {
        if let Some(text) = app.yank_range(lo, hi, field) {
            app.apply_yank(text);
        }
    }
    app.clear_visual();
}

fn focused_strip(app: &App) -> Option<StripKind> {
    match app.focus {
        Focus::ChipStrip => Some(StripKind::Filter),
        Focus::ExcludeStrip => Some(StripKind::Exclude),
        Focus::HighlightStrip => Some(StripKind::Highlight),
        _ => None,
    }
}

/// Palette titles/icons from [`ActionId::meta`] (design.md table).
pub fn palette_spec(id: ActionId) -> Option<(&'static str, &'static str)> {
    let m = id.meta();
    if m.in_palette {
        Some((m.palette_title, m.palette_icon))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Focus;
    use crate::export::ExportSource;
    use crate::filter_model::TimeBound;
    use crate::keymap::ActionKind;
    use crate::theme;

    fn idle_app() -> App {
        App::new(100)
    }

    #[test]
    fn in_palette_set_matches_design_table() {
        let expected: Vec<(ActionId, &str, &str)> = vec![
            (
                ActionId::GlobalFilterNew,
                "Add Filter",
                theme::GLYPH_TITLE_FILTER,
            ),
            (
                ActionId::GlobalHighlightNew,
                "Find Highlight",
                theme::GLYPH_TITLE_HIGHLIGHT,
            ),
            (
                ActionId::GlobalHighlightAdd,
                "Add Highlight",
                theme::GLYPH_TITLE_HIGHLIGHT,
            ),
            (
                ActionId::GlobalExcludeNew,
                "Add Exclude",
                theme::GLYPH_TITLE_EXCLUDE,
            ),
            (ActionId::GlobalOpenHelp, "Open Help", theme::GLYPH_HELP),
            (ActionId::GlobalQuit, "Quit", theme::GLYPH_QUIT),
            (
                ActionId::LogListWrapToggle,
                "Toggle Wrap",
                theme::GLYPH_TITLE_LOG,
            ),
            (
                ActionId::LogListDetailFields,
                "Show Fields",
                theme::GLYPH_VIEW_FOCUS,
            ),
            (
                ActionId::LogListDetailPretty,
                "Show Pretty",
                theme::GLYPH_VIEW_FOCUS,
            ),
            (
                ActionId::LogListClearLive,
                "Clear Live Buffer",
                theme::GLYPH_DISCONNECT,
            ),
            (
                ActionId::LogListResumeFollow,
                "Resume Following",
                theme::GLYPH_FOLLOWING,
            ),
            (
                ActionId::LeaderManage,
                "Manage Filters",
                theme::GLYPH_MODE_MANAGE,
            ),
            (
                ActionId::LeaderPresetSave,
                "Save Preset",
                theme::GLYPH_MODE_NEW,
            ),
            (
                ActionId::LeaderPresetOpen,
                "Open Preset",
                theme::GLYPH_SOURCE_DIR,
            ),
            (
                ActionId::LeaderSummary,
                "Show Summary",
                theme::GLYPH_TITLE_DASHBOARD,
            ),
            (
                ActionId::OpenFile,
                "Open File",
                theme::GLYPH_SOURCE_OPEN_FILE,
            ),
            (ActionId::OpenStream, "Open Stream", theme::GLYPH_SOURCE_HDC),
            (ActionId::TimeSet, "Set Time Window", theme::GLYPH_TIME),
            (ActionId::TimeClear, "Clear Time Window", theme::GLYPH_TIME),
            (ActionId::LockPid, "Lock PID", theme::GLYPH_LOCK),
            (ActionId::LockTid, "Lock TID", theme::GLYPH_LOCK),
            (
                ActionId::LockViewHighlight,
                "View Focus Highlight",
                theme::GLYPH_VIEW_FOCUS,
            ),
            (
                ActionId::LockViewSevere,
                "View Focus Severe",
                theme::GLYPH_CRASH,
            ),
            (ActionId::LockClear, "Clear Lock", theme::GLYPH_LOCK),
            (ActionId::BookmarkAdd, "Add Bookmark", theme::GLYPH_BOOKMARK),
            (
                ActionId::BookmarkRemove,
                "Remove Bookmark",
                theme::GLYPH_BOOKMARK,
            ),
            (
                ActionId::BookmarkManage,
                "Manage Bookmarks",
                theme::GLYPH_BOOKMARK,
            ),
            (ActionId::YankCli, "Yank CLI", theme::GLYPH_TITLE_LOG),
            (
                ActionId::LogListYankMsgLine,
                "Yank Message",
                theme::GLYPH_FIELD_MSG,
            ),
            (
                ActionId::StripDDelete,
                "Delete Selected Group",
                theme::GLYPH_TITLE_EXCLUDE,
            ),
            (
                ActionId::StripDDisable,
                "Toggle Selected Group",
                theme::GLYPH_ACTION_TOGGLE_OFF,
            ),
        ];
        assert_eq!(
            expected.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            PALETTE_IDS
        );
        let seen: std::collections::HashSet<ActionId> = ActionId::ALL
            .iter()
            .copied()
            .filter(|id| id.meta().in_palette)
            .collect();
        let expected_set: std::collections::HashSet<ActionId> =
            PALETTE_IDS.iter().copied().collect();
        assert_eq!(seen, expected_set);
        for &id in PALETTE_IDS {
            assert_eq!(
                id.meta().kind,
                ActionKind::Leaf,
                "prefixes stay out of palette"
            );
        }
        for (id, title, icon) in expected {
            assert_eq!(palette_spec(id), Some((title, icon)), "{id:?}");
        }
        assert!(!ActionId::GlobalCommandPalette.meta().in_palette);
        for &id in &[
            ActionId::LogListMoveDown,
            ActionId::LogListLeader,
            ActionId::GlobalFocusFilter,
            ActionId::YankTag,
            ActionId::ChipFieldMsg,
        ] {
            assert!(!id.meta().in_palette, "{id:?} must stay out of palette");
        }
    }

    #[test]
    fn live_hides_time_keeps_clear_live() {
        let mut app = idle_app();
        app.export_source = ExportSource::Hdc { device: None };
        let ids: Vec<ActionId> = catalog(&app).iter().map(|i| i.id).collect();
        assert!(!ids.contains(&ActionId::TimeSet));
        assert!(!ids.contains(&ActionId::TimeClear));
        assert!(ids.contains(&ActionId::LogListClearLive));
    }

    #[test]
    fn no_current_row_hides_lock_pid_and_add_bookmark() {
        let app = idle_app();
        assert!(app.current_row().is_none());
        let ids: Vec<ActionId> = catalog(&app).iter().map(|i| i.id).collect();
        assert!(!ids.contains(&ActionId::LockPid));
        assert!(!ids.contains(&ActionId::BookmarkAdd));
        assert!(!ids.contains(&ActionId::LogListYankMsgLine));
        assert!(ids.contains(&ActionId::GlobalFilterNew));
    }

    #[test]
    fn empty_filter_strip_omits_strip_delete() {
        let mut app = idle_app();
        app.focus = Focus::ChipStrip;
        let ids: Vec<ActionId> = catalog(&app).iter().map(|i| i.id).collect();
        assert!(!ids.contains(&ActionId::StripDDelete));
        assert!(!ids.contains(&ActionId::StripDDisable));
    }

    #[test]
    fn resume_follow_hidden_while_following() {
        let mut app = idle_app();
        app.following = true;
        let ids: Vec<ActionId> = catalog(&app).iter().map(|i| i.id).collect();
        assert!(!ids.contains(&ActionId::LogListResumeFollow));
        app.following = false;
        let ids: Vec<ActionId> = catalog(&app).iter().map(|i| i.id).collect();
        assert!(ids.contains(&ActionId::LogListResumeFollow));
    }

    #[test]
    fn time_clear_hidden_without_bound() {
        let mut app = idle_app();
        app.export_source = ExportSource::File("/tmp/x.log".into());
        assert!(!when(&app, ActionId::TimeClear));
        app.time_bound = Some(TimeBound {
            since: Some("10:00:00".into()),
            until: None,
        });
        // Still no date catalog → TimeSet hidden; TimeClear visible.
        assert!(when(&app, ActionId::TimeClear));
        assert!(!when(&app, ActionId::TimeSet));
    }

    #[test]
    fn empty_query_yields_no_catalog_rows() {
        let app = idle_app();
        assert!(filtered_catalog(&app, "").is_empty());
        let hits = filtered_catalog(&app, "filter");
        assert!(
            hits.iter().any(|i| i.id == ActionId::GlobalFilterNew),
            "{hits:?}"
        );
    }

    #[test]
    fn dispatch_filter_new_opens_picker() {
        let mut app = idle_app();
        dispatch(&mut app, ActionId::GlobalFilterNew);
        let session = app.picker.as_ref().expect("picker");
        assert!(matches!(session.kind, PickerKind::Filter));
        assert!(matches!(session.mode, crate::picker::PickerMode::New));
    }

    #[test]
    fn dispatch_highlight_add_force_new_with_existing_groups() {
        use crate::highlight_model::HighlightGroup;
        let mut app = idle_app();
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
        dispatch(&mut app, ActionId::GlobalHighlightAdd);
        let session = app.picker.as_ref().expect("picker");
        assert!(matches!(session.kind, PickerKind::Highlight));
        assert!(matches!(session.mode, crate::picker::PickerMode::New));
        assert!(!session.auto_from_manage);
    }

    #[test]
    fn dispatch_highlight_new_opens_manage_when_groups_exist() {
        use crate::highlight_model::HighlightGroup;
        let mut app = idle_app();
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
        dispatch(&mut app, ActionId::GlobalHighlightNew);
        let session = app.picker.as_ref().expect("picker");
        assert!(matches!(session.kind, PickerKind::Highlight));
        assert!(matches!(session.mode, crate::picker::PickerMode::Manage));
    }

    #[test]
    fn dispatch_move_down_advances_cursor() {
        let mut app = idle_app();
        // empty visible: move is a no-op but must not panic
        dispatch(&mut app, ActionId::LogListMoveDown);
        dispatch(&mut app, ActionId::LogListMoveUp);
    }

    #[test]
    fn lock_clear_omitted_when_only_view_focus() {
        let mut app = idle_app();
        app.view_focus.highlight = true;
        assert!(
            !when(&app, ActionId::LockClear),
            "fu / Clear Lock does not clear view focus"
        );
        app.lock_pid = Some("1".into());
        assert!(when(&app, ActionId::LockClear));
    }

    #[test]
    fn dispatch_time_set_noop_on_live() {
        let mut app = idle_app();
        app.export_source = ExportSource::Hdc { device: None };
        dispatch(&mut app, ActionId::TimeSet);
        assert!(app.time_panel.is_none());
    }

    #[test]
    fn unbound_palette_row_still_catalogued() {
        let mut app = idle_app();
        app.keymap = crate::keymap::merge_user_toml(
            r#"
[global]
filter_new = ""
"#,
        )
        .unwrap();
        let item = catalog(&app)
            .into_iter()
            .find(|i| i.id == ActionId::GlobalFilterNew)
            .expect("unbound Add Filter still listed");
        assert!(item.key_hint.is_empty());
    }
}
