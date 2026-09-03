# Command Palette + ActionStore

> Executable contracts for the VS Code-style command palette (`C-p`) and
> `action::dispatch` (task `08-14-tui-command-palette`).

---

## Scenario: Intent-command palette over ActionStore

### 1. Scope / Trigger

Update this spec when changing:

- `action.rs` (`when` / `catalog` / `filtered_catalog` / `dispatch`)
- `command_palette.rs` session state
- `C-p` open binding, `[command_palette]` keymap section
- Palette render (`ui.rs::render_command_palette`)
- Which `ActionId`s are `in_palette`

**Out of scope here**: PickerSession, idle status two-hint set (see
[status-help.md](./status-help.md)), CLI `alnav grep`.

### 2. Signatures

| Item | Location | Contract |
|------|----------|----------|
| `ActionMeta.in_palette` / `palette_title` / `palette_icon` | `keymap.rs` | Catalog metadata; titles English; icons `theme::GLYPH_*` |
| `action::when(app, id) -> bool` | `action.rs` | `ActionMeta::allowed(file_mode)` AND extra predicates |
| `action::catalog(app) -> Vec<PaletteItem>` | `action.rs` | `PALETTE_IDS` order; omit when `when` is false (never dim) |
| `action::filtered_catalog(app, query)` | `action.rs` | Empty query → **empty vec** (do not call `fuzzy_label_indices` empty path) |
| `action::dispatch(app, id)` | `action.rs` | `match` on `ActionId`; not `Box<dyn Fn>` |
| `CommandPalette { query: TextField, selected }` | `command_palette.rs` | No paint, no dispatch |
| `PALETTE_VISIBLE_ROWS` | `command_palette.rs` (`10`) | Dropdown viewport cap |
| `App.command_palette` | `app.rs` | `Option<CommandPalette>` |
| `App::command_palette_available` | `app.rs` | LogList/strips, Normal, no other modal |
| `App::open_command_palette` | `app.rs` | Clears pending + visual; `following=false`; no-op if unavailable |
| `App::close_command_palette` | `app.rs` | Sets `None`; **does not** `resume_following` |
| `GlobalCommandPalette` | `keymap.rs` | Default `C-p` ; `in_palette = false` |
| `PaletteSubmit` / `PaletteUp` / `PaletteDown` / `PaletteClose` | `keymap.rs` | `[command_palette]` section; not in catalog |
| `theme::GLYPH_TITLE_PALETTE` | `theme.rs` | Palette shell title glyph |

### 3. Contracts

**Authority split**

- `action.rs`: what an action does, palette catalog, `when`.
- `keymap.rs`: `ActionId` / `ActionMeta` / bindings / `KeymapStore::display`.
- Do not add a second `CommandId` enum. Do not store handlers as closures.

**Open / close**

- Open only when `command_palette_available()`: `Focus` ∈ {LogList, ChipStrip, ExcludeStrip, HighlightStrip}, `Mode::Normal`, and none of picker / time / detail / highlight-edit / help / summary / dashboard / open-file / stream / preset-name / compare tray / already-open palette.
- Pending chords **are** allowed: open clears them (`c` then `C-p` opens the palette).
- Esc / Ctrl+C → `close_command_palette()` (same as Help/Detail: no resume follow).
- Enter with hits → close, then `dispatch(selected id)`. Zero hits → Enter no-op, stay open.
- Dashboard `C-p` is consumed by `dispatch_global_chords` but
  `command_palette_available()` is false while `dashboard` is set, so open
  is a no-op. Do not open the palette over the startup Dashboard.

**Widget**

- New control. **Do not** reuse `PickerSession`.
- Top-centered `top_modal_rect`; width `min(72, max(40, frame.width * 60 / 100))`.
- Empty query: input shell only (`render_modal_shell` + `GLYPH_TITLE_PALETTE`).
- Hits: list via `stack_below_rect_gapped`, at most `PALETTE_VISIBLE_ROWS`, ViewportPaint.
- Zero hits: one dim row `No matching commands` using `theme::candidate_unselected_style()`.
- Row: icon + `palette_title` + right-aligned key hint (`keymap.display`, empty if unbound).
- Search haystack is `palette_title` only (`fuzzy_label_indices`). No aliases, no Preview, no MRU.
- Query: `TextField` + `text_field::apply_key`. **Up/Down move the list; `j`/`k` type.** Left/Right are caret.

**`when` extras** (after `allowed`)

| ActionId | Extra |
|----------|--------|
| Fields / Pretty / BookmarkAdd / Yank Message | current row exists |
| LockPid / LockTid | current row has non-empty pid/tid |
| BookmarkRemove | current row is bookmarked |
| TimeSet | file mode **and** date candidates |
| TimeClear | file mode **and** `time_bound` set |
| Resume Following | `following == false` |
| LockClear | `lock_pid` or `lock_tid` (not view-focus alone) |
| Strip delete/disable | focused strip has a selected group |
| ClearAllRules | any Filter / Highlight / Exclude group exists |

`TimeSet` dispatch must also no-op on live (`is_file_mode()`), not only hide in the catalog.

**Help / status**

- Help catalog includes `GlobalCommandPalette` (`C-p` / `palette`).
- Idle status bar stays two hints (`? help`, `; filter`). Do **not** add idle `C-p`.
- Palette open → `help_available` false; status L2 is the palette context.
- `BookmarkManage` palette title is **Open Compare Panel** (same `ActionId`; dispatch opens `App.compare`, not a picker).
- `LeaderManage` palette title is **Manage Rules** (Filter + Highlight + Exclude).
- `ClearAllRules` palette title is **Clear All Rules**; hidden when those three lists are empty. Dispatch only opens confirm (`App.confirm`); it does not open Manage.
- Highlight catalog: **Find Highlight** (`GlobalHighlightNew`, `/`) is find-or-create; **Add Highlight** (`GlobalHighlightAdd`, unbound) is always `open_picker_new`. Do not merge them back into one dispatch.

### 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| `C-p` while Detail/Picker/Help/Time/Compare open | Palette does not open |
| `C-p` while `pending_chip` | Opens palette; pending cleared |
| Empty query Enter | No-op |
| Zero-match Enter | No-op; dim empty row stays |
| Live `TimeSet` via dispatch | No-op (do not open time panel) |
| Palette Esc / Ctrl+C | Close; `following` unchanged |
| Source switch | `reset_for_source_switch` closes palette |

### 5. Good / Base / Bad Cases

- **Good**: Idle LogList `C-p` → input-only shell; type `filter` → `Add Filter`; Enter ≡ `;`.
- **Base**: Type `bookmark` including `k` without moving selection; Up/Down change the row.
- **Bad**: Empty query listing all intent commands; `j`/`k` moving the list; opening over Detail; idle status gaining a third `C-p` hint; `HashMap<ActionId, Box<dyn Fn>>`.

### 6. Tests Required

- `action::` — `PALETTE_IDS` matches `in_palette`; live hides Time; no row hides Lock PID / Add Bookmark; empty strip hides delete; empty rules hide Clear All Rules; `dispatch(GlobalFilterNew)` opens Filter New; `dispatch(GlobalHighlightNew)` is find-or-create; `dispatch(GlobalHighlightAdd)` force-New; `dispatch(TimeSet)` no-op on live.
- `command_palette::` — `k` types into query; `move_sel` clamps.
- `help::` — catalog/LogList include palette; idle status still `help`+`filter`; `help_available` false when palette open.
- `ui::` — empty open has no `Add Filter`; query `filter` shows it; no `Color::*` / inline glyphs in non-test `ui.rs`.
- `keymap::` — `--init` contains `[command_palette]` and `command_palette = "C-p"`.

### 7. Wrong vs Correct

#### Wrong

```rust
// Empty query uses fuzzy_label_indices' first-N path → dumps the catalog
fuzzy_label_indices(&titles, "")
```

```rust
// j/k move the list — "Bookmark" cannot be typed
KeyCode::Char('k') => palette.move_sel(1, n)
```

#### Correct

```rust
if query.is_empty() {
    return Vec::new();
}
fuzzy_label_indices(&titles, query)
```

```rust
// j/k go through apply_key into TextField; only Up/Down call move_sel
```

---

## Design Decision: ActionStore vs KeymapStore

**Context**: Keymap already had `ActionId`, but handlers lived in `main.rs` `km_code` branches.

**Options**: parallel `CommandId`; closures in a HashMap; catalog+`dispatch` over the same `ActionId`.

**Decision**: Same `ActionId`. `dispatch` is a `match`. Palette is a filtered view (`in_palette` + `when`). Keymap stays bindings-only.

**Extensibility**: New intent command = `ActionId` + meta (including palette fields) + one `dispatch` arm + `when` if needed. Do not fork a second ID enum.
