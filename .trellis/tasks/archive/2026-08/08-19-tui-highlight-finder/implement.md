# Implement: Highlight find-or-create picker

## Checklist

1. **App open + activate**
   - Add `App::open_highlight_finder` and `App::activate_highlight_group`.
   - Enable-before-jump; file scan restart when enabling or switching active.

2. **Action / keymap split**
   - `GlobalHighlightNew` dispatch → `open_highlight_finder`; meta detail + palette title **Find Highlight**.
   - Add `GlobalHighlightAdd` (`toml_key = highlight_add`, no default key, palette **Add Highlight**) → `open_picker_new(Highlight)`.
   - Put `GlobalHighlightAdd` in `PALETTE_IDS` (keep Find in catalog too, or replace Add’s old slot — both titles must exist).
   - Update `action::` palette snapshot tests.

3. **Highlight Manage branch**
   - `picker_render_data`: `PickerKind::Highlight` + Manage → pattern-only list, Jump actions, highlight preview. Do not call `unified_picker_items`.
   - `handle_picker_key` Manage: dedicated Highlight arm (not the Unified `_` toggle path).
   - Helpers: `highlight_visible_indices` / selected index (directory-structure naming).
   - Auto-New on zero matches; draft-empty returns to Manage only when `auto_from_manage`.
   - Extend `PickerSession` with that flag; `enter_new` stays wipe-for-manual-New.

4. **Highlight New Enter**
   - New/Edit Enter: `submit_draft` + existing push/update/jump. Stop `confirm_or_submit` on New.
   - Empty-draft New: no history candidate paint or Down-move-through-history.

5. **Help copy**
   - Highlight + Picker blurbs; `GlobalHighlightNew` hint detail; tests that mention “force New” / `/` opens New when groups exist.

6. **Tests** (map to ACs)
   - Rewrite `handle_normal_key '/'` tests: empty → New; nonempty groups → Manage.
   - New: Highlight Manage Enter activates, enables, jumps, closes; other groups stay enabled.
   - Unified Enter still toggles, no jump, picker stays.
   - Auto-New + draft-clear back to Manage.
   - Palette/dispatch `GlobalHighlightAdd` force New with existing groups.
   - New `er` vs existing `error` creates `er`; `ERROR` reuses.
   - Disabled group + Enter enables then jumps.
   - Help `/` still does not open the Highlight picker.

7. **Validate** (commands below)

## Validation commands

```bash
cargo test -p alnav --bin alnav app::
cargo test -p alnav --bin alnav action::
cargo test -p alnav --bin alnav help::
cargo test -p alnav --bin alnav -- highlight
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

## Risky files / rollback

| File | Risk |
|------|------|
| `alnav/src/main.rs` | Highlight Manage falling through Unified toggle if the kind branch is incomplete |
| `alnav/src/picker.rs` | `enter_new` clearing the draft before auto-New copies query |
| `alnav/src/keymap.rs` | `--init` / palette snapshot miss `highlight_add` |
| `alnav/src/app.rs` | Activate without enable → silent jump no-op |

Rollback: revert the four source files + tests; no on-disk format.

## Follow-up before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` written
- [x] User asked to start execution after Approach A + Action Plan
- [x] Curate `implement.jsonl` / `check.jsonl` (this step)
