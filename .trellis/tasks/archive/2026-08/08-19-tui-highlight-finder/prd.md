# Highlight find-or-create picker

## Goal

Make `/` the fast path to **find an existing highlight and start viewing it** (set the n/N target and jump to the first hit). Creating a new pattern is the fallback when nothing matches. Unified Manage (`Space Space`) stays the collection editor: Enter still toggles enable/disable.

## Background / Confirmed Facts

- Highlight groups paint matching text and do not hide rows. `n`/`N`, underline, and first-hit jump follow `App.active_highlight` only. Enable/disable only changes which patterns paint. `fh` (view focus) is a separate AND filter (`session-filters.md`).
- LogList `/` is `ActionId::GlobalHighlightNew` → `open_picker_new(PickerKind::Highlight)` (`action.rs:134`, `keymap.rs` `highlight_new`). Command palette title is **Add Highlight** via `.with_palette`.
- Highlight New reuses `HighlightBox`: empty draft lists existing patterns; typing shows vocab; Enter uses `confirm_or_submit`, which prefers a fuzzy-matched existing group (`highlight_model.rs:153-174`, `main.rs:1598-1631`). That is the only “search existing tags → activate + jump” path, and it is buried inside New.
- Unified Manage Enter toggles Filter / Highlight / Exclude (`main.rs:2016-2028`). It does not set `active_highlight` or jump.
- `PickerKind::Highlight` in Manage currently falls through the Unified item builder (`unified_picker_items` lists Filter+Highlight+Exclude). Bookmark/Preset already have kind-specific Manage branches (`directory-structure.md` “Picker session dispatch”).
- Manage no-match does **not** auto-switch to New (`manage_no_match_stays_in_manage`). Help copy that says otherwise is stale (`help.rs:849`).
- `jump_first_match_of` no-ops when the group is disabled (`app.rs:2914-2916`). Activate must enable first, then jump.
- Grill 2026-08-19 locked Approach A: `/` = Highlight-only find-or-create; Unified Enter stays toggle; keep other highlights painted; do not auto-enable `fh`; palette **Add Highlight** stays force New.

## Requirements

### R1 — `/` is Highlight find-or-create

- LogList `/` (`GlobalHighlightNew`) opens `PickerKind::Highlight` only (not Unified).
- No highlight groups → New (same as today).
- One or more groups → Manage listing **only** those patterns (label = pattern, no `[Highlight]:` prefix).
- Type to nucleo-fuzzy filter the pattern list (same `filtered_indices` as other Manage panels).

### R2 — Highlight Manage Enter activates and views

- Enter on a selected existing group: enable if disabled → set `active_highlight` → jump first visible hit (`jump_first_match_of`) → close picker → focus LogList.
- Other enabled highlights stay painted. Do not toggle them off. Do not set `view_focus.highlight`.
- Esc / Ctrl+C close the picker and do not `resume_following`.
- Ctrl-X edits the selected pattern (existing Edit path). Delete / Ctrl-Backspace delete with confirm (Highlight-only, no Tab multi-select).
- Unified Manage (`Space Space`) is unchanged: Highlight rows still Enter-toggle, stay open, no jump.

### R3 — Create fallback

- Highlight Manage + non-empty query + zero matches → auto New; query becomes the New draft.
- From that auto-New: emptying the draft while groups still exist → return to Manage (query cleared). Manual palette Add / empty-list New does not fall back to Manage when the draft is cleared.
- Command palette **Add Highlight** stays force New (`open_picker_new`). `/` and Add Highlight must not share that force-New dispatch.
- `;` (Filter New), `` ` `` (Exclude New), and Unified Manage auto-New behavior stay unchanged.

### R4 — New panel no longer embeds history search

- Highlight New empty draft does not list existing patterns as candidates.
- Typing still requests vocab; Tab still replaces the last token.
- Enter compiles the draft (`submit_draft`). Exact ignore-case reuse still goes through `push_or_find_highlight_group` + jump. Enter must not steal a fuzzy prefix (e.g. draft `er` must not become existing `error`).

### R5 — Help / keymap copy

- Help Highlight + Picker blurbs and key details: `/` is find-or-create, not “force New”.
- `GlobalHighlightNew` detail/label match find-or-create. Palette **Add Highlight** remains an add/create command.
- Help `/` while Help is open stays Help search and must not open the Highlight picker.

## Acceptance Criteria

- [ ] AC1: With no highlights, LogList `/` opens Highlight New; Enter on `error` creates the group, sets `active_highlight`, jumps, and closes the picker.
- [ ] AC2: With existing highlights, LogList `/` opens Highlight-only Manage (patterns only). Typing filters them. Enter on a match enables it if needed, sets `active_highlight`, jumps to the first hit, and closes. Other enabled groups stay enabled.
- [ ] AC3: Unified Manage Enter on a Highlight row still toggles enabled and leaves the picker open; `active_highlight` and cursor are unchanged.
- [ ] AC4: In Highlight Manage, a query with zero matches switches to New with that text as draft; clearing the draft returns to Manage when groups exist.
- [ ] AC5: Command palette **Add Highlight** always opens Highlight New, even when groups already exist.
- [ ] AC6: Highlight New Enter on `er` while `error` exists creates `er` (does not activate `error`). Exact ignore-case `ERROR` reuses the existing group and jumps.
- [ ] AC7: Help closed: LogList `/` is the finder. Help open: `/` is Help search. Highlight/Picker Help copy no longer says `/` force-New.
- [ ] AC8: Enter on a disabled highlight enables it, then jumps (no silent no-op from `jump_first_match_of`).
- [ ] AC9: `fh` / other paints / Filter/Exclude `;` `` ` `` / Unified no-match staying in Manage are unchanged.
- [ ] AC10: `cargo test -p alnav --bin alnav` and `cargo fmt -p alnav --check` pass.

## Out of Scope

- Solo mode (disable every other highlight on activate).
- Auto-enabling view focus (`fh`).
- Changing Filter/Exclude bare New keys or Unified Manage Enter-toggle.
- Restoring Unified Manage → New auto-switch.
- New keybindings such as `//` or `s`.
- Windows-only behavior.

## Open Questions

None. Grill 2026-08-19 locked Approach A and the Action Plan; user asked to create the Trellis task and start execution.
