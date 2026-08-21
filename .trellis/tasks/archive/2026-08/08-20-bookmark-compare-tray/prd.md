# Bookmark compare tray

## Goal

Turn session bookmarks from “jump waypoints in add order” into a **compare tray**: pin a few log rows as owned snapshots, keep a one-line summary on the log, and open a large panel to view them in log-time order with Δt so they can be compared without scrolling the main list.

## Background / Confirmed Facts

- Today `Bookmark` is `row_id` + auto `label` (timestamp/level/tag/msg, no pid/tid). Storage order is insert order. The log-top strip shows the newest 3 (`display_recent`); `mm` Manage lists newest-first (`bookmark_visible_indices` in `main.rs`). Sorting never uses the timestamp in the label.
- Cap is `BOOKMARK_SOFT_CAP = 50`. `ma` on an existing id flashes `EXISTS` (not toggle). `md` removes the current LogList row’s pin. Jump uses `row_id` → `visible`; `JumpResult::{Ok,Evicted,Filtered}`. Live eviction and file filter both make Enter fail.
- LogList already highlights pinned origins (`bookmark_row_style`) and minimap marks alive pins. Export (`yc`) does not include bookmarks. Session-only; process exit discards.
- `mm` is `ActionId::BookmarkManage` (Bookmark context, default `m` after pending `m`). It opens `PickerKind::Bookmark` (fzf Manage, Delete confirm). That picker is the wrong surface for side-by-side compare.
- Grill 2026-08-20 locked the compare-tray shape: snapshots, one-line summary, on-demand large panel, minimal panel keys. Full LogList keymap reuse in the panel was proposed then **explicitly rejected**.

## Requirements

### R1 — Snapshot pins

- `ma` toggles the current LogList row: not pinned → copy an owned snapshot and pin; already pinned → remove. Flash `BOOKMARKED` / `REMOVED`. Empty row → `NO ROW`.
- Snapshot fields at pin time: `row_id`, `timestamp`, `level`, `pid`, `tid`, `tag`, `pkg`, `msg`, `raw` (enough to render like a log line and yank). Do not keep a live pointer as the display source.
- Cap **16**. Full → reject, flash `BOOKMARKS FULL`. Do not drop existing pins.
- `md` on LogList still removes the pin for the current origin row (`NOT BOOKMARKED` if none).
- Visual-line ranges do not bulk-pin.
- Origin yellow bg + minimap marks stay for **alive** `row_id`s. Evicted origins lose bg/minimap; the snapshot remains in the tray.

### R2 — Sort and Δt

- Display order (summary span, panel list) is log time ascending, then `row_id`.
- Sort key reuses `LogEntry::time_full` / `time_hms` rules on the snapshot timestamp. No parseable time → after all timed pins (stable by `row_id`).
- Δt is relative to the **previous timed pin** in that order. First timed pin has no Δt. Untimed pins show `—`. Format short (`+1.2s`, `+3m`, `+1h2m`). Same-day vs cross-day does not change Δt meaning.

### R3 — One-line summary

- Replace the 3-row newest strip. When there is at least one pin, Log top shows **one** line, e.g. `★ 3  10:01:02→10:01:08`.
- Span uses the min/max **timed** pins. Same calendar day (or no date in the stamps) → `HH:MM:SS→HH:MM:SS`. Different days → include `MM-DD` (or `YYYY-MM-DD` when the stamp has a year). Zero timed pins → `★ N` only.
- Empty tray → line hidden (folded). Not a focus region; not clickable. Open the panel with `mm`.

### R4 — Compare panel (`mm`)

- Zero pins: do **not** open a panel; flash `NO BOOKMARKS`.
- Non-empty: open a large rounded modal (covers the main log). `following = false`. Esc / Ctrl+C close the panel and do **not** `resume_following`.
- Rows: same wrap + search/highlight paint as the main log (`render_entry_lines`), including pid/tid. No line numbers. Always multi-line wrap; ignore `App.collapsed_view` / `w`.
- Row prefix: Δt (or `—`); if the origin is not currently jumpable, also `☆` (filtered and evicted share the mark). Snapshot text stays full intensity.
- Keys **only**: `j`/`k` (by pin, not visual wrap line), `g`/`G` (first/last pin; `g` is a single key like LogList, not a `gg` chord), `yy` (yank selected snapshot `raw` via existing yank/clipboard/`YANKED`), `dd` (pending `d` then `d` deletes selected pin, no confirm; Esc cancels pending), `Enter`, `Esc`.
- `Enter`: if origin is in current `visible`, jump (`following = false`), close panel, focus LogList. If alive but filtered → `BOOKMARK NOT VISIBLE`. If gone → `BOOKMARK EVICTED`. Do not clear filters.
- `dd` to zero pins closes the panel.
- No search, chip, lock, detail, visual, paging, `J`/`K`, wheel, `Y`, `ma`/`md` inside the panel. Further ops happen on LogList after Enter.
- Live ingest may continue under the modal; jumpability/`☆` may update. Snapshot **content** does not.

### R5 — Help / keymap copy

- `BookmarkManage` is “open compare panel”, not Manage/fzf. Help + status L2 (`pending_m` and panel-open) match the new keys. Palette title follows the same wording.
- `PickerKind::Bookmark` Manage/New/Edit is no longer the `mm` surface. Dead picker branches and newest-first helpers go away with the tests that encoded them.

### R6 — Quality

- No `alnav-core` behavior change except reusing existing `time_full`/`time_hms` via `LogEntry` on the snapshot.
- Theme: new prefix/summary styles through `theme.rs` tokens. No `Color::*` in `ui.rs`.
- `bookmark_row_ids` stays in lockstep with pin mutations (including panel `dd` and `ma` toggle).
- English flash/Help. Modal Ctrl+C = close panel.

## Acceptance Criteria

- [ ] AC1: `ma` pins a snapshot with pid/tid/msg; second `ma` on that row unpins. 17th distinct row flashes `BOOKMARKS FULL` and does not insert.
- [ ] AC2: Pins display in log-time order (not add order). Untimed pins sit last. Adjacent timed pins show Δt vs the previous timed pin; untimed show `—`.
- [ ] AC3: Log top is one summary line `★ N` plus time span as specified; empty tray folds it. Opening `mm` with pins does not keep the old 3-newest wrapped strip.
- [ ] AC4: `mm` with 0 pins flashes `NO BOOKMARKS` and opens nothing. With pins, the large panel lists all pins (up to 16) using log rendering + Δt/`☆` prefixes.
- [ ] AC5: Panel `j`/`k`/`g`/`G` move by pin; `yy` yanks that snapshot’s raw; `dd` deletes without confirm; last `dd` closes. `Enter` jumps when visible and closes; filtered/evicted flash and stay in the panel. `Esc`/Ctrl+C close without resume following.
- [ ] AC6: Panel ignores `/`, `w`, `J`, chip/lock/detail. `w` on the main list does not collapse panel rows.
- [ ] AC7: Visual selection does not add pins. `md` on LogList still removes by current origin `row_id`.
- [ ] AC8: Help/status/`BookmarkManage` copy say compare/open panel, not Manage. `mm` does not open the Bookmark fzf picker.
- [ ] AC9: `cargo test -p alnav --bin alnav` and `cargo fmt -p alnav --check` pass. Bookmark picker newest-first / confirm-delete tests are rewritten or removed.

## Out of Scope

- Persistence, named pin sets, notes, ±N context lines.
- LogList-spawn full keymap in the panel.
- Bulk-pin from visual mode; clear-all chord; click-to-open summary.
- `MM` dead key; inventing a new `Space m` Leader binding (none exists today).
- Including pins in `yc` export.
- Changing CLI / `alnav-core` filter or parse behavior.
- Windows-only behavior.

## Open Questions

None. Grill 2026-08-20 locked the MVP. User asked to create this Trellis task after freezing the spec.
