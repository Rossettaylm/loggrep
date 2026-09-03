# Directory Structure

> How alnav backend code is organized in this project.

---

## Overview

alnav is a ratatui/crossterm TUI built atop `alnav-core`. Modules
are flat under `src/`, each owning one concern of the state machine, render
pipeline, or input dispatch. Cross-module state flows through `App`.

---

## Directory Layout

```
alnav/src/
├── main.rs         # CLI entry, terminal lifecycle, event loop, key dispatch
├── app.rs          # App state machine: store/Visible::{All,Subset}/groups/time_bound/bookmarks/compare/picker/focus
├── store.rs        # RowStore/FileStore/StreamStore/RowRef (mmap file + stream)
├── scan.rs         # File Vis+Inc highlight worker + severe prefetch (async-scans)
├── model.rs        # EntryRow: owned line model, from_line()/from_line_or_raw()/as_log_entry()
├── keymap.rs       # ActionId / ActionMeta / KeymapStore (bindings + --init); palette titles/icons live on ActionMeta
├── action.rs       # ActionStore: when / catalog / filtered_catalog / dispatch (match, not closures)
├── command_palette.rs # CommandPalette session (TextField query + selected); no paint, no dispatch
├── filter_model.rs # Group/GroupList + TimeBound (global window matching)
├── fuzzy.rs        # TUI nucleo-matcher facade (Search/Filter/Picker; see fuzzy-matching.md)
├── candidate_match.rs # Async vocab fuzzy for Picker New (gen+cancel)
├── time_panel.rs   # `-f` ts panel: date candidates from rows + HH:MM:SS clamp
├── highlight_model.rs # HighlightGroup/HighlightGroupList (pattern + fuzzy, no Regex)
├── picker.rs       # PickerSession/PickerKind(incl. Preset)/PickerMode/UnifiedKind/UnifiedItem
├── input.rs        # ChipField/Chip/InputBox/Popup (Enter two-phase)
├── ui.rs           # Render: log list, strips, picker, minimap, modals, time panel
├── palette.rs      # Palette, name fold, mix, contrast_fg
├── theme_builtins.rs # nine Palette constants
├── theme.rs        # UiTokens + style fns (mapped from Palette; CLI logcolor is separate)
├── bookmark.rs     # Bookmark snapshot + BookmarkList + ComparePanel + JumpResult
├── preset.rs       # Named Filter/Exclude/Highlight presets (`presets/*.toml`)
├── help.rs         # HintEntry L1/L2; two-level Help (Home+7 pages, `/` search); FAST_SCROLL_STEP
├── export.rs       # H10 yc CLI export (filters + lock + time_bound)
├── config.rs       # theme.toml overlay + config.toml (incl. theme =)
├── preview.rs      # H1 preview sampling (stream rows or file lazy parse)
└── ingest.rs       # spawn_live_ingest (ADB/HDC DropOldestRing) / IngestHandle; file ingest tests-only
```

### ActionStore vs KeymapStore vs command palette

`keymap.rs` owns identity and keys (`ActionId`, `ActionMeta`, `KeymapStore`).
`action.rs` owns what an action **does** (`dispatch`) and which intent
commands appear in the palette (`when`, `catalog`, `filtered_catalog`).
`command_palette.rs` is session state only. Paint is `ui.rs::render_command_palette`.
Do **not** reuse `PickerSession` for the palette. See
[command-palette.md](./command-palette.md).

### Global source / preset chords

`OpenFile` / `OpenStream` / `LeaderPresetSave` / `LeaderPresetOpen` /
`GlobalCommandPalette` are `KeyContext::Global` leaves. Defaults:

| Action | Default | Notes |
|--------|---------|-------|
| Open File | `C-f` | Dashboard bare `o` still opens the file panel |
| Open Stream | `C-g` | |
| Preset Save | `C-s` | |
| Preset Open | `C-o` | |
| Command Palette | `C-p` | Dashboard: consumed, but `open_command_palette` no-ops |

`main.rs::dispatch_global_chords` is the single matcher. Call it from
`handle_normal_event` **and** `handle_dashboard_key` (after Ctrl+C quit).
`OpenFile` / `OpenStream` pass `from_dashboard: app.dashboard.is_some()`
so Esc returns to Dashboard.

Do **not** bind default chords as `C-S-<letter>`: Cursor/VS Code steals
`C-S-o` / `C-S-l`, and traditional TTYs often drop Shift so the exact
modifier match never fires. Analysis operators (`c`/`C`/`y`/`f`/`t`/`mm`/`dd`)
stay two-stage. Retired: `of`/`os`, `LogListOpen` / `pending_open` /
`KeyContext::Open`.

### Global time window module

`time_panel.rs` owns the `ts` editor only. Matching and persistence live on
`App.time_bound` (`TimeBound` in `filter_model.rs`). See
[session-filters.md](./session-filters.md).


---

## Module Organization

### Picker session dispatch (Manage-by-kind)

`PickerSession` carries a `kind: PickerKind` and `mode: PickerMode`. The
Manage mode is dispatched **by `session.kind`** in two places:

- `picker_render_data` (`main.rs`): builds the candidate list per kind.
  `Unified` aggregates Filter+Highlight+Exclude only (no bookmarks).
  `Highlight` Manage is also per-kind: pattern-only labels,
  `ActionKind::Jump`, never `unified_picker_items`. Future per-kind
  Manage panels branch here.
- `handle_picker_key` Manage branch (`main.rs`): routes keys per kind.
  `Unified` supports Tab multi-select + Ctrl-X edit + Ctrl-K clear-all
  rules; Enter = toggle (Highlight rows included — no jump, picker
  stays open). Destructive confirm lives on `App.confirm`
  (`DeleteMany` / `DeletePreset` / `ClearAll`), is drawn screen-centered,
  and is handled before picker keys. `ClearAll` wipes Filter + Highlight
  + Exclude only (not lock / time / bookmarks), then closes to LogList
  with `following=false`. The same confirm is also opened by `C-p`
  `ClearAllRules` without opening Manage.
  `Highlight` (finder): no Tab multi-select; Enter =
  `activate_highlight_group` + close; Ctrl-X = edit; Delete /
  Ctrl-Backspace = delete confirm; nonempty query + zero hits →
  `enter_new_with_draft` (`auto_from_manage`); nonempty query that is
  not an exact existing pattern (ignore-case) appends a trailing create
  row (`GLYPH_MODE_NEW` + query); Down can select it; Enter on that row
  creates the query and closes; last group deleted → New.
  `Preset` is Manage-only (no auto-New): Enter = apply, Ctrl-X = rename
  name dialog, Delete = `ConfirmKind::DeletePreset`; save is `C-s`
  outside the picker.
  Bookmarks are **not** a `PickerKind`. `mm` / `BookmarkManage` opens
  `App.compare` (`ComparePanel`); do not reintroduce `PickerKind::Bookmark`.

**Convention**: to add a new per-kind Manage panel, add a `PickerKind`
variant, branch in both `picker_render_data` and `handle_picker_key`, and
provide a `*_visible_indices`/`*_selected_index` helper pair. Do NOT
reintroduce a `UnifiedKind` variant for it — `UnifiedKind` is the
aggregate-panel item taxonomy only.

### Bookmark compare tray (not a picker)

`App.compare: Option<ComparePanel>` is a dedicated modal (same event-loop
layer as Help). `Bookmark` is an owned `EntryRow` snapshot plus jump
`row_id`. Display order is `BookmarkList::sorted_indices()` (log time
then `row_id`; untimed last). Cap is 16. `ma` toggles; `mm` opens the
panel or flashes `NO BOOKMARKS`. Handle compare keys **before** LogList;
Esc / Ctrl+C close without `resume_following`. `help_available` is false
while the panel is open.

### Bookmark row-id cache

`App.bookmark_row_ids: HashSet<u64>` mirrors `BookmarkList.items` row_ids
for O(1) LogList bg lookup. It is mutated in lockstep with every
`BookmarkList` mutation (`bookmark_add_current` toggle, `bookmark_remove_current`,
`compare_delete_selected` → `delete_bookmark_at_index`, `clear_bookmarks`).
Any new mutation site on `BookmarkList` MUST sync this set —
`render_log_list` reads it every frame.

---

## Naming Conventions

- `*_indices` helpers return indices into the backing `Vec` (not display
  order). When display order differs from storage (e.g. bookmark compare
  time-sort), the helper maps display→storage internally and returns
  storage indices.
- `PickerKind` = which panel kind; `PickerMode` = Manage/New/Edit within
  that panel; `UnifiedKind` = item taxonomy inside the Unified aggregate
  panel only.
- theme.rs accessors are `pub fn <thing>_style()` / `pub fn <thing>_color()`;
  glyphs are `pub const GLYPH_*`.

---

## Examples

- `BookmarkList::sorted_indices` / `App::compare_selected_storage_index`:
  log-time display order, maps back to `bookmarks.items` storage index.
- `unified_picker_items` (`main.rs`): aggregate for the Unified panel only
  (Filter+Highlight+Exclude; bookmarks are the compare tray, not picker rows).
- `highlight_visible_indices` (`main.rs`) + `App::open_highlight_finder` /
  `activate_highlight_group`: LogList `/` find-or-create. Enable before
  jump (`jump_first_match_of` no-ops when disabled). Does not set
  `view_focus`. Palette **Add Highlight** is `GlobalHighlightAdd` →
  `open_picker_new`.
