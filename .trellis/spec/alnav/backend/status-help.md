# Status Bar + Help Panel

> Executable contracts for the English status bar and read-only Help (`?`).

---

## Overview

`help.rs` owns keybinding **labels/details** as structured `HintEntry` data.
**Key strings** come from `App.keymap` (`keymap.toml` / builtin registry).
The status bar and Help panel both render from that source — do not maintain
a second Chinese/`key:label` string table or hard-code key glyphs in `ui.rs`.

---

## 1. Scope / Trigger

Update this spec when changing:

- Status-bar left icons / right L1–L2 hints
- Help panel open/close/scroll keys
- Flash toast language on the status bar
- `?` keybinding availability

---

## 2. Signatures

| Item | Location | Contract |
|------|----------|----------|
| `HintEntry { key, label, detail }` | `help.rs` | `key` from `KeymapStore::display`; Status uses `key`+`label`; Help uses `detail` |
| `context_kind(app) -> ContextKind` | `help.rs` | modal/confirm > pending > focus |
| `context_entries(app) -> Vec<HintEntry>` | `help.rs` | Full L1 or L2 set — status L2 + Home Active source (do not shrink) |
| `status_hint_entries(app) -> Vec<HintEntry>` | `help.rs` | Status bar subset: idle LogList/Strip curated 1–2 keys; else full |
| `keymap.toml` / `KeymapStore` | `keymap.rs` | Startup deep-merge; `--init` serializes defaults including `[help]` search/back |
| `context_hint_spans(app, max)` | `help.rs` | Consumes `status_hint_entries`; dim key + label; gap `"  "`; no `:`/`\|` |
| `help_available(app) -> bool` | `help.rs` | Gate for opening Help; **false** when command palette or compare tray is open |
| `ContextKind::CommandPalette` | `help.rs` | Palette open → status L2 is palette keys (Esc/Enter/Up/Down) |
| `ContextKind::Compare` | `help.rs` | Compare tray open → status L2 is `j/k` `g/G` `yy` `dd` Enter Esc (not Manage) |
| `GlobalCommandPalette` | `keymap.rs` | Default `C-p`; listed on Overlays + LogList Active, **not** idle status |
| `HOME_ACTIVE_LIMIT` / `HelpPage` / `HelpView` / `HelpSearch` | `help.rs` | Home Active ≤4; seven zone pages; `/` search session |
| `help_body_lines(app)` | `help.rs` | Home: Active + TOC + chrome; Page: title + blurb + keys + chrome |
| `page_blurb` / `page_doc_lines` | `help.rs` | Zone contract (≤5 English lines) + that page’s key table |
| `FAST_SCROLL_STEP` | `help.rs` (`pub const`, value `7`) | Shared by LogList `J`/`K` and Help `J`/`K` (Home: TOC; Page: body) |
| `App.help_open` / `help_view` / `help_search` | `app.rs` | Panel state; `close_help` does **not** `resume_following` |
| `handle_help_key` | `main.rs` | Esc/`?`/Ctrl+C close whole panel; `h`/Backspace → Home; `1`–`7` jump; `/` search |
| `help_modal_rect` | `ui.rs` | Vertically centered Help shell (`centered_modal_rect`) |
| `theme::help_search_hit_style` / `help_search_current_style` | `theme.rs` | Help substring highlight (reuse highlight ramp; no `Color::*` in `ui.rs`) |
| `status_pill` / `status_pill_value` / `status_icon_dim` / `status_flash_pill` | `theme.rs` | Status-bar left cluster + flash; on-pill fg via `contrast_fg` |
| `status_icon` / `status_icon_value` / `status_soft` | `theme.rs` | Kept for non-status-bar callers |

---

## 3. Contracts

### Status bar three zones (single row)

Left (never yields) → middle flash pill → pad + right-aligned hints.

| State | Render |
|-------|--------|
| follow | Always a slot: on = `status_pill` success; off = `status_icon_dim` (same glyph, DIM, no fill) |
| device | Always a slot: live connected = source glyph accent pill; live `ingest_done` = `GLYPH_DISCONNECT` warning pill; `-f` = file glyph accent pill (never disconnect) |
| lock / time / view focus / progress | When active: `status_pill_value` — no LOCK/TIME word prefix; view focus uses `GLYPH_VIEW_FOCUS` + `HL`/`ERR` |
| visual | When active: accent `status_pill` — no VISUAL word |
| highlight hits | Search glyph + `k/total` as accent pill_value — **no** `[brackets]` |
| cursor `n/N` | Dim text, not a pill |
| pending prefixes | **Dropped** (`c…` / `SPC…` etc. are not in the left cluster) |
| flash | Middle filled pill (`status_flash_pill`); `FAILED` → warning fill, else success; 3s via `set_flash` |

### Status bar right hints

- English only; key dim, label normal weight; entries separated by spaces only.
- Idle **LogList / LogListLive**: exactly `? help` and `; filter` (from keymap via `status_hint_entries`).
- Idle **ChipStrip / ExcludeStrip / HighlightStrip**: exactly `? help` and `d del…`.
- Operator-pending and modal (Picker / Time / Detail / Confirm / Highlight-edit / Input / Leader / **CommandPalette** / **Compare**): full `context_entries`.
- Do **not** add idle `: palette` / `C-p palette` — Open Command Palette is Help-catalog only.
- Help Home Active uses the first **4** of `context_entries`; the full list still feeds status L2 and must not shrink.
- Hints hide first when budget `< MIN_HELP_WIDTH` (8); flash keeps a ~12-column floor (`FLASH_MIN`) while visible.

### Help panel (`?`)

- **Read-only** — never executes commands / never replaces Picker or the command palette.
- Open when: focus ∈ {LogList, ChipStrip, ExcludeStrip, HighlightStrip} AND no picker/time/detail/highlight edit/**command palette**/**compare tray** AND no `pending_*` / `pending_leader`.
- **Two-level**: Home (Active title + ≤4 `context_entries` + numbered TOC `1`–`7` + chrome footer) and seven zone pages (Filter / Exclude / Highlight / Log / Session / Picker / Overlays). Each page: title + ≤5-line English contract + that zone's key table. No eighth Help-keys page — chrome lives on the Home footer.
- Opening Help preselects TOC from `Focus` (strips → matching page, LogList → Log).
- Close any layer: Esc / `?` / Ctrl+C → `close_help()`; does **not** resume following. `h` / Backspace return to Home (restore TOC highlight); on Home they are no-ops. Digits `1`–`7` jump from Home or any page except while the search prompt is active.
- Home `j`/`k`/`J`/`K` move the TOC (`J`/`K` = `FAST_SCROLL_STEP`). Sub-page `j`/`k`/`J`/`K`/`g`/`G` scroll that page's body; max scroll is `line_count - viewport` so the last line sits at the bottom, not the top.
- Placement: Help uses `centered_modal_rect` (vertically centered). Input/Search/Time/Detail stay `top_modal_rect`.
- `/` search (Help context only): ignore-case substring over Home + all pages; vim prompt (`TextField`); highlight via `theme::help_search_*`. Prompt: printable chars edit the query (including `j`/`k`/`h`/`1`–`7`/`n`); Up/Down walk hits; Enter nonempty commits (keep highlights, then `n`/`N`); Enter empty or Esc in prompt / after committed hits clears search and stays in Help. No match: flash `NO MATCH`, do not jump. LogList `/` is Highlight **find-or-create** (`open_highlight_finder`) when Help is closed.
- Short Home frames pin Active + chrome and keep at least one TOC row when height allows.

### Keybinding note

- `?` opens Help. `/` is Highlight find-or-create (`open_highlight_finder`) when Help is closed (empty groups → New; else Highlight-only Manage). Help `/` is search (`KeyContext::Help`) and must not steal LogList `/`. Palette **Add Highlight** (`GlobalHighlightAdd`) is force New. `C-p` opens the command palette (not Help).
- Do **not** rebind `?` to Highlight find-or-create.
- LogList L1: `f` label is `focus` (lock + view focus); L2_LOCK includes `p`/`t`/`h`/`e`/`u`.
- L2_TIME: `t` set / `u` clear (open key is `tt`, not `ts`). Catalog session: `f h/e`, `t t/u`.
- Source switch: `C-f` Open File, `C-g` Open Stream (not `of`/`os`, not `C-S-o`/`C-S-l`). Dashboard bare `o` still opens the file panel.
- Presets: `C-s` save / `C-o` open (not `Space w` / `Space o`).
- Bookmarks: `mm` opens the compare tray (`BookmarkManage` copy = compare/open panel, not Manage). `pending_m` L2 is add / delete / compare.

### Flash language

All `set_flash` / TimePanel flash strings that appear on the status bar are
English. Prefer short uppercase tokens (`EXISTS`, `NO ROW`, `UNKNOWN FIELD`).

---

## 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| `?` while `pending_yank` (etc.) | Help does not open (pending handler consumes key) |
| `?` while Picker/Time/Detail/Compare open | Help does not open |
| Help Esc | Close whole panel; `following` unchanged (`h`/Backspace go Home; search Esc clears search first) |
| Help `/` while prompt open | Types into the query (does not open Highlight finder) |
| Narrow terminal | Right hints hide when budget `< MIN_HELP_WIDTH` (8); left icons win |

---

## 5. Good / Base / Bad Cases

- **Good**: LogList Normal `?` → Home; `4` opens Log; `J` scrolls +7; Esc closes; still not following.
- **Base**: Wide idle LogList status shows `? help  ; filter` without colons, not the long L1 list.
- **Bad**: Reintroducing `j/k:移` Chinese colon strings, or `FOLLOWING` word badges, or `?` → Highlight New.

---

## 6. Tests Required

- `help::` — context kind priority, live L1 for HDC and ADB (no `t`, has `^L`), idle status spans are `help`+`filter` (not `j/k move`), pending chip lists `tag`/`msg`, Home Active + TOC (no “All commands”), blurbs ≤5, Filter page lists filter-new, Log owns wrap/visual/yank, `FAST_SCROLL_STEP` matches Log jump text, `/CHIP` substring hits, `page_max_scroll` keeps last line at bottom.
- `ui::` status bar — match stats without `[]`; wide idle shows help+filter not `j/k`; follow glyph when paused; pending has no `c…`; flash pill visible with pending L2; narrow keeps follow glyph and hides hints. Help Home shows numbered TOC; Exclude page shows blurb; short frame keeps Active + chrome + a TOC row; search hit uses theme style; Help modal is vertically centered; page render clamps scroll to viewport.

---

## 7. Wrong vs Correct

#### Wrong

```rust
// Separate Chinese status string + hard-coded Help paragraphs
const L1: &str = "j/k:移 Esc:随";
app.set_flash("已存在");
theme::status_badge(GLYPH_FOLLOWING, "FOLLOWING", success());
```

#### Correct

```rust
// Shared HintEntry; status subset vs full Help; English flash pill
status_hint_entries(app); // idle: help + filter
context_entries(app);     // full L1 for status L2; Home Active takes first 4
app.set_flash("EXISTS");
theme::status_pill(GLYPH_FOLLOWING, success()); // on
theme::status_icon_dim(GLYPH_FOLLOWING);        // off
theme::status_flash_pill("EXISTS");
// Help J/K shares help::FAST_SCROLL_STEP with LogList
```

---

## Design Decision: Single Hint Source

**Context**: Status bar and Help must stay consistent after English redesign.

**Decision**: `help.rs` is the only keybinding and Help-contract copy source. `context_entries` stays full for status L2; Home Active takes the first 4. Zone blurbs and page key tables live in `help.rs`. UI only styles/spans.

**Why**: Prevents Help catalog from shrinking when the status bar curates idle hints, and keeps dim-key rendering data-driven.
