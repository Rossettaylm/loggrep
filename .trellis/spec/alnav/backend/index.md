# Backend Development Guidelines

> Best practices for alnav backend / TUI state machine work.

---

## Overview

Executable contracts for the TUI crate. Prefer these over re-deriving
behavior from CLAUDE.md when implementing filters, pickers, or modals.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Directory Structure](./directory-structure.md) | Module layout + picker/compare tray/time_panel/ActionStore ownership | Active |
| [Command Palette + ActionStore](./command-palette.md) | `C-p` palette, `when`/`catalog`/`dispatch` | Active |
| [Session Filters](./session-filters.md) | Lock + global `App.time_bound` contracts | Active |
| [TUI Fuzzy Matching](./fuzzy-matching.md) | nucleo-matcher contracts for Picker/Filter/Highlight | Active |
| [Stream Visible + Live Ingest](./stream-visible-ingest.md) | `Visible::All` / `Subset` + drop-oldest ADB/HDC ring | Active |
| [FileStore mmap](./file-store.md) | `-f` mmap + lazy parse + bg filter | Active |
| [Async Vis Scans](./async-scans.md) | Highlight Inc / severe prefetch / LogList loading | Active |
| [Status Bar + Help](./status-help.md) | English status bar, two-level `?` Help + `/` search, `FAST_SCROLL_STEP` | Active |
| [TUI UX Boosters](./tui-ux-boosters.md) | Crash detail (`P`), Summary panel (`Leader i`), disconnect icon, wrap toggle (`w`) | Active |
| [TUI Theme System](./theme-system.md) | Palette-driven themes, `config.toml` `theme`, TUI-only (CLI keeps `logcolor`) | Active |
| [Quality Guidelines](./quality-guidelines.md) | Forbidden patterns, testing requirements | Active |
| [Database Guidelines](./database-guidelines.md) | N/A for this crate | Stub |
| [Error Handling](./error-handling.md) | Error types, handling strategies | Stub |
| [Logging Guidelines](./logging-guidelines.md) | Structured logging, log levels | Stub |

---

## Pre-Development Checklist

- [ ] Read [session-filters.md](./session-filters.md) before changing filter/lock/time matching or `yc` export.
- [ ] Read [fuzzy-matching.md](./fuzzy-matching.md) before changing TUI text match, Highlight paint, Filter chip evaluation, or **candidate panel / vocab / Picker list** paths (Candidate panel SLOs).
- [ ] Read [stream-visible-ingest.md](./stream-visible-ingest.md) before changing `visible`, `push_row` eviction, or live ingest.
- [ ] Read [file-store.md](./file-store.md) before changing `-f` ingest, `RowStore`, or file filter scanning.
- [ ] Read [async-scans.md](./async-scans.md) before changing File highlight/`nN`/severe/minimap cache paths or LogList loading.
- [ ] Read [quality-guidelines.md](./quality-guidelines.md) Forbidden Patterns (theme colors, Group.time, modal Ctrl+C) and "Popup chrome vs strip chrome".
- [ ] Read [status-help.md](./status-help.md) before changing status-bar hints, flash language, or Help (`?`) keys/scroll.
- [ ] Read [tui-ux-boosters.md](./tui-ux-boosters.md) before changing `DetailView::Pretty`'s crash branch, `SummaryView`/`summary_gen`, the disconnect icon, or `App.collapsed_view`.
- [ ] Read [theme-system.md](./theme-system.md) before changing `theme.rs` / `palette.rs` / `theme.toml` / `config.toml` `theme`.
- [ ] Touching picker Manage: read Directory Structure "Picker session dispatch". Bookmarks use the compare tray (`App.compare`), not `PickerKind::Bookmark`.
- [ ] Touching bookmarks / `mm` / `ma`: read Directory Structure "Bookmark compare tray" and keep `bookmark_row_ids` in lockstep.
- [ ] Read [command-palette.md](./command-palette.md) before changing `C-p` / `action::dispatch` / palette catalog / `when`.
- [ ] Read Directory Structure "Global source / preset chords" before changing `C-f`/`C-g`/`C-s`/`C-o` or Dashboard key dispatch. Do not default-bind `C-S-<letter>`.
- [ ] Severe log tag/msg paint goes through `theme::severe_entry_style` (see [theme-system.md](./theme-system.md)); do not hard-code red in `ui.rs`.

## Quality Check

- [ ] `Group` has no `time` field; time is on `App.time_bound`.
- [ ] Interactive time keys gated on `is_file_mode()`.
- [ ] Stream `visible` is `Visible::All` (no identity `Vec` + per-row index shift).
- [ ] File filter hits use `Visible::Subset` (line numbers); no full-file owned `EntryRow` / `matched`.
- [ ] `--hdc` and `--adb` ingest use the same drop-oldest ring (`INGEST_RING_CAP`), not a blocking/unbounded channel.
- [ ] File highlight stats / `n`/`N` use hit index (no UI O(visible) `row_at`); FilterBatch does not full-parse.
- [ ] New modal key paths handle Ctrl+C as cancel when appropriate.
- [ ] Command palette is not a `PickerSession`; empty query lists nothing; idle status has no third `C-p` hint.
- [ ] Bookmark `mm` is `App.compare`, not a picker; `help_available` is false while compare is open; Esc/Ctrl+C close without `resume_following`; `bookmark_row_ids` stays synced on `ma` toggle and panel `dd`.
- [ ] Popup shells stay rounded; strips stay divider; confirm uses the same `picker_area` as the picker.
- [ ] TUI paint goes through `theme::*` tokens (palette-mapped). CLI colored output stays on `alnav::logcolor`.
- [ ] Status hints / Help copy stay in `help.rs` (`HintEntry`); two-level Help (Home + 7 pages); Esc/`?`/Ctrl+C close without `resume_following`; `h`/Backspace back; LogList/Help `J`/`K` share `FAST_SCROLL_STEP`.
- [ ] `cargo test -p alnav` green; `cargo fmt -p alnav --check` clean.

---

**Language**: All documentation should be written in **English**.
