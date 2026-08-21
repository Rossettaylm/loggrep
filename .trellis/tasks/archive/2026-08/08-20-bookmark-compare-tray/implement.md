# Implement: Bookmark compare tray

## Checklist

1. **Model (`bookmark.rs`)**
   - Snapshot storage (`EntryRow` or equivalent); cap 16; drop `display_recent` / `BOOKMARK_DISPLAY_N`.
   - `sorted_indices`, Δt helper, summary line (same-day HMS vs cross-day date).
   - Unit tests for cap, toggle-related list ops, sort, Δt, span, untimed last.

2. **App**
   - `ma` toggle; keep `md`; sync `bookmark_row_ids` on every mutation including panel delete.
   - `ComparePanel` + `open_compare_panel` / `close_compare_panel` (clear pendings, no resume following).
   - Panel `yy` → `apply_yank(raw)`; `dd` by sorted index; Enter reuses `jump_to_bookmark`.
   - `help_available` false while compare open.

3. **Dispatch (`action.rs` / `main.rs` / `keymap.rs`)**
   - `BookmarkManage` → open compare panel; update ActionMeta/palette/help strings.
   - Modal key handler (swallow); do not open `PickerKind::Bookmark`.
   - Delete Bookmark picker arms + `ConfirmKind::DeleteBookmark` if unused.
   - Status `ContextKind` for the panel.

4. **UI (`ui.rs` / `theme.rs`)**
   - One-line summary in log top.
   - Large `render_modal_shell` compare list: `render_entry_lines` + prefix; ignore `collapsed_view`.
   - `compare_delta_style` (or reuse muted) in `theme.rs` only.

5. **Help**
   - Bookmark / pending_m / panel copy; tests that mention Manage/fzf/`m:管理`.

6. **Tests → ACs**
   - AC1 toggle + cap; AC2 order/Δt; AC3 summary; AC4 mm empty vs panel; AC5 keys/jump/dd/yy; AC6 no fallthrough `/`/`w`; AC7 no visual bulk pin; AC8 no Bookmark picker; AC9 cargo test/fmt.

7. **Validate** (commands below)

## Validation commands

```bash
cargo test -p alnav --bin alnav bookmark::
cargo test -p alnav --bin alnav app::
cargo test -p alnav --bin alnav help::
cargo test -p alnav --bin alnav -- bookmark
cargo test -p alnav --bin alnav
cargo fmt -p alnav --check
```

Do not `cargo fmt --all` (touches `alnav-core`).

## Risky files / rollback

| File | Risk |
|------|------|
| `alnav/src/bookmark.rs` | Sort/Δt on mixed/empty timestamps; summary cross-day |
| `alnav/src/app.rs` | `ma` toggle vs HashSet; panel cursor after delete |
| `alnav/src/main.rs` | Modal keys leaking to LogList; leftover Bookmark picker path |
| `alnav/src/ui.rs` | Prefix vs wrap width; summary vs old 3-line strip tests |
| `alnav/src/keymap.rs` / `help.rs` | Stale “manage” copy; `--init` dump |

Rollback: revert TUI crate files; no on-disk format.

## Follow-up before `task.py start`

- [x] `prd.md` / `design.md` / `implement.md` written
- [x] User explicitly approves this planning summary (required; create ≠ start)
- [x] Curate `implement.jsonl` / `check.jsonl`
