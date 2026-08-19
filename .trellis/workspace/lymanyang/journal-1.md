# Journal - lymanyang (Part 1)

> AI development session journal
> Started: 2026-07-22

---



## Session 1: Picker mid-cursor editing + hardware caret

**Date**: 2026-07-23
**Task**: Picker mid-cursor editing + hardware caret
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Implemented TextField mid-cursor editing for all Picker drafts with Manage key remaps (Ctrl-X / Delete / Ctrl-Backspace); then switched draft caret to terminal hardware cursor via Frame::set_cursor_position.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `37007f0` | (see git log) |
| `b7fe30b` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 2: TUI global time window (-f)

**Date**: 2026-07-24
**Task**: TUI global time window (-f)
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Grilled and shipped App.time_bound with ts/tu panel (date candidates from rows, HH:MM:SS clamp); hdc hard-hide; filter_active/yc/TIME badge; Trellis session-filters spec; 351 tests green.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `fcded9b` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 3: Grill split hdc/mmap Trellis plans

**Date**: 2026-07-24
**Task**: Grill split hdc/mmap Trellis plans
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Grilled mmap perf task; chose S1+P-after drop-oldest for hdc; B-gate for file; split into 07-24-tui-hdc-stream-visible then 07-24-tui-mmap-file-backend; planning artifacts written; not started.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

(No commits - planning session)

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 4: HDC Visible::All + drop-oldest ring

**Date**: 2026-07-24
**Task**: HDC Visible::All + drop-oldest ring
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Implemented and checked Visible::All (O(1) eviction) plus P-after hdc DropOldestRing CAP=8192; committed; archived tui-hdc-stream-visible; mmap sibling remains planning.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `95a701f` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 5: mmap FileStore land + All-scan grill

**Date**: 2026-07-24
**Task**: mmap FileStore land + All-scan grill
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Committed mmap FileStore (RowStore, bg filter, Subset). Grilled All-scan A1 (Vis/Inc, LogList L1+T+Free); next: new tui-file-async-scans task.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `0dca14d` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 6: File async Vis scans

**Date**: 2026-07-24
**Task**: File async Vis scans
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Background Vis+Inc highlight hits, filter UI-parse reinforcement, severe prefetch, LogList L1+T+Free loading; archived tui-file-async-scans.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `2dffcda` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 7: TUI popup rounded borders + compact picker

**Date**: 2026-07-27
**Task**: TUI popup rounded borders + compact picker
**Package**: aloggrep-core
**Branch**: `master`

### Summary

Restored rounded four-sided popup chrome (modal/candidate/preview) with 1-cell gaps; half-width picker when preview hidden; confirm dialog anchors to actual picker frame. Spec: popup vs strip chrome contract.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `c895275` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 8: TUI status bar + Help panel

**Date**: 2026-07-27
**Task**: TUI status bar + Help panel
**Package**: aloggrep-core
**Branch**: `master`

### Summary

English status bar (icon badges, dim-key L1/L2), read-only ? Help with Active+catalog and J/K fast scroll; captured status-help.md code-spec.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `490de5c` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 9: Picker vocab async-cancel match

**Date**: 2026-07-31
**Task**: Picker vocab async-cancel match
**Package**: alnav-core
**Branch**: `master`

### Summary

Moved Picker New vocab fuzzy matching off the UI thread with generation-based cancel so paste/fast typing stays responsive; updated fuzzy-matching spec.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `42b9557` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 10: TUI live stream auto-reconnect

**Date**: 2026-08-10
**Task**: TUI live stream auto-reconnect
**Package**: alnav-core
**Branch**: `master`

### Summary

Implemented hdc/adb auto-reconnect with disconnect icon, device probe + health check to avoid false RECONNECTED, keep buffer on reconnect; archived 08-10-tui-live-reconnect.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `4124096` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 11: 候选面板检索一级指标

**Date**: 2026-08-13
**Task**: 候选面板检索一级指标
**Package**: alnav-core
**Branch**: `master`

### Summary

落地 ResultCap=256、ViewportPaint、Arc vocab snapshot、Preview 节流，并将 Candidate panel SLOs 写入 fuzzy-matching/quality-guidelines；任务归档。

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `552a6f9` | (see git log) |
| `3d7e2ed` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 12: TUI open-file log_dirs nucleo search

**Date**: 2026-08-13
**Task**: TUI open-file log_dirs nucleo search
**Package**: alnav-core
**Branch**: `master`

### Summary

Open file 改为 log_dirs 异步语料 + nucleo Pattern::parse；移除 path_complete；配置 log_dirs/log_extensions；任务归档。

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `52fea12` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 13: TUI command palette (C-p)

**Date**: 2026-08-14
**Task**: TUI command palette (C-p)
**Package**: alnav
**Branch**: `master`

### Summary

Shipped a VS Code-style command palette on C-p: ActionStore owns catalog/when/dispatch, palette searches titles with nucleo, LogList and strips open it, Enter dispatches after close. 611 alnav tests green. Merged feat/tui-command-palette into master.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `4f1f109` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 14: Severe row red + Global C-f/C-g

**Date**: 2026-08-14
**Task**: Severe row red + Global C-f/C-g
**Package**: alnav
**Branch**: `master`

### Summary

Painted E/F/crash tag+msg in theme error red. Moved Open File/Stream to C-f/C-g (Ctrl+Shift is stolen/unreliable) and made Global chords work on Dashboard; presets stay C-s/C-o.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `5e4b89c` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 15: Two-level Help panel with search

**Date**: 2026-08-17
**Task**: Two-level Help panel with search
**Package**: alnav-core
**Branch**: `master`

### Summary

Shipped two-level Help (Home + seven zone pages with design contracts), global ignore-case substring search/highlight, Esc-closes/h-back split, and centered layout without bottom overscroll.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `5a5a5eb` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete


## Session 16: Highlight find-or-create picker

**Date**: 2026-08-19
**Task**: Highlight find-or-create picker
**Package**: alnav-core
**Branch**: `master`

### Summary

LogList / is now Highlight find-or-create: search existing tags, Enter activates and jumps. Unified Manage Enter stays toggle; palette Add Highlight stays force New.

### Main Changes

- Detailed change bullets were not supplied; see the summary above.

### Git Commits

| Hash | Message |
|------|---------|
| `6100fc0` | (see git log) |

### Testing

- Validation was not recorded for this session.

### Status

[OK] **Completed**

### Next Steps

- None - task complete
