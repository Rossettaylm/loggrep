# Quality Guidelines

> Code quality standards for alnav.

---

## Overview

These standards are derived from CLAUDE.md "UI 设计指导" and decisions
captured in tasks under `.trellis/tasks/`. They are executable rules, not
aspirations.

---

## Forbidden Patterns

### Don't: hardcode colors in render code

```rust
// DON'T — inline Color in ui.rs / main.rs
item.style(Style::default().bg(Color::Rgb(54, 46, 0)))
```

**Why**: breaks theme.toml / config.toml `theme` overrides. TUI colors
come from installed `UiTokens` (palette-mapped). CLI colored output
still uses `alnav::logcolor` and must not be restyled from TUI tokens.

**Instead**:
```rust
item.style(theme::bookmark_row_style())   // reads UiTokens.bookmark_row_bg
```

### Don't: reintroduce `enabled` on entities that have no enable semantics

`Bookmark.enabled` was a zombie field (set on add, toggleable in picker,
but no consumer ever gated on it). It was deleted in `07-23-bookmark-ux`.
Do not re-add `enabled` to a model unless a real consumer gates on it.

### Don't: route a per-kind Manage panel through `UnifiedKind`

`UnifiedKind` is the aggregate-panel item taxonomy only. A dedicated
Manage panel (e.g. Bookmark) keys off `session.kind` + a `*_visible_indices`
helper, never `unified_selected_id`/`UnifiedId`.

### Don't: put session time bounds on `Group`

Global `--since`/`--until` and interactive `ts` live on `App.time_bound`.
Re-attaching time to `Group` makes `di`/`dd` accidentally disable the window
and diverges from CLI global-AND semantics. See
[session-filters.md](./session-filters.md).

### Don't: let modal Ctrl+C fall through as a typed character

When a top modal owns key routing (Time panel, Help, command palette, compare tray, etc.),
Ctrl+C must cancel like Esc at the `KeyEvent` layer — otherwise `Char('c')`
is inserted into the draft or quits the app while a modal is open.

### Don't: duplicate keybinding copy outside `help.rs`

Status-bar L1/L2 and the Help panel share `HintEntry` data in `help.rs`. Do
not reintroduce Chinese `key:label` string constants or a second Help
paragraph table in `ui.rs`. See [status-help.md](./status-help.md).

### Don't: resume following when closing Help

`close_help()` must not call `resume_following` — same as Detail Esc,
`close_command_palette()`, and `close_compare_panel()`.

### Don't: reuse PickerSession for the command palette

The `C-p` palette is `App.command_palette` + `command_palette.rs`. Do not
route it through `PickerKind` / `UnifiedKind`. Empty query must not list
commands; `j`/`k` type into the query (Up/Down move). See
[command-palette.md](./command-palette.md).

### Don't: reuse PickerSession for the bookmark compare tray

`mm` / `BookmarkManage` opens `App.compare` (`ComparePanel`). Do not
reintroduce `PickerKind::Bookmark`, `ConfirmKind::DeleteBookmark`, or
newest-first `bookmark_visible_indices`. Do not route pins through
`UnifiedKind`.

---

## Required Patterns

### Per-frame O(1) lookups need a cache

`render_log_list` runs every frame over the viewport. Per-row predicates
that would be O(n) (e.g. "is this row bookmarked?") MUST be backed by a
`HashSet`/`HashMap` on `App`, synced at every mutation site.

Example: `App.bookmark_row_ids: HashSet<u64>` → `is_bookmark_row()` O(1).

### Picker Manage-by-kind dispatch

New per-kind Manage panels branch in BOTH `picker_render_data` (build
labels/actions) AND `handle_picker_key` (key routing). See
`directory-structure.md` "Picker session dispatch".

### Action icons via `ActionKind`, not ad-hoc spans

Candidate rows that have a primary action (Enter) show a right-flush
nerdfont icon via `candidate_label_spans(action, area_width)`. Do not
append raw icon spans in callers.

### Popup chrome vs strip chrome

| Surface | Chrome |
|---------|--------|
| Filter / Exclude / Highlight strip | `divider_block` (top+bottom only) |
| Log region | `rounded_block` |
| Popup shell (`render_modal_shell`) | `popup_block` → `rounded_block(..., true)` + `border_style(true)` |
| Standalone candidate popup | `render_candidate_list(..., bordered=true)` |
| Candidate list inside Picker left | `bordered=false` (outer shell already borders) |

Adjacent popups leave `POPUP_GAP` (1 cell): vertical via
`stack_below_rect_gapped`, Picker L/R via `split_picker_lr_gapped`.

`picker_frame_rect(frame, show_preview)`: full width when preview is on;
≈ half width (centered) when off. `render_confirm_dialog` MUST receive
that same `picker_area` — never recompute a full-width frame on its own.

Do NOT reintroduce divider-only shells for popups, or nest a bordered
candidate list inside an already-bordered Picker pane.

### Candidate panel ResultCap / ViewportPaint

- Every candidate narrowing exit truncates to `fuzzy::CANDIDATE_RESULT_CAP` (256).
  Empty query must not materialise a full vocab/table for the UI.
- `render_candidate_list` must not build `ListItem` + `fuzzy_char_indices` for
  the entire labels vec — only the viewport (`candidate_viewport_range`).
- Vocab New matching reuses an Arc snapshot per scope; do not re-clone the
  full Msg cache on the UI thread every keystroke.
- Batch fuzzy over many haystacks uses `fuzzy::FuzzyScorer` (one Pattern/Matcher
  per query). Do not call `fuzzy_score` in a hot loop over vocab entries.

---

## Testing Requirements

- Every removed field/arm gets its test deleted or rewritten to the new
  contract (e.g. `mm` opens compare not Bookmark picker;
  `toggle_unified_enabled_bookmark` deleted).
- New behavioral contract (jump, delete, bg priority) gets a test that
  fails on a plausible regression.
- `cargo test --workspace` must be green before commit; `cargo fmt -p
  alnav --check` must be clean. Do NOT run `cargo fmt --all` — it
  touches `alnav-core` (out of scope for tui tasks).

---

## Code Review Checklist

- [ ] No `Color::*` literals in new render code (theme.rs only).
- [ ] New `BookmarkList`/`HashSet` mutation sites sync the cache.
- [ ] Picker changes branch on `session.kind` in render AND key dispatch.
- [ ] Popup surfaces use rounded `render_modal_shell`; strips stay `divider_block`.
- [ ] Confirm dialog anchors to the actual `picker_frame_rect(..., show_preview)`.
- [ ] Deleted fields have no surviving references (grep).
- [ ] `cargo test --workspace` green; `cargo fmt -p alnav --check` clean.
