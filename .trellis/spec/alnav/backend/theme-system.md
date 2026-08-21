# TUI Theme System

> Palette-driven TUI themes with semantic token overlays. CLI (`alnav grep`)
> is unchanged.

---

## Overview

alnav's TUI ships one default look (today's chrome + ANSI log colors) and
eight named palettes. A theme is an 18-slot ANSI palette. A fixed mapping
table produces every UI and log color the TUI paints. Users pick a palette
in `config.toml` and may overlay slots or semantic tokens in `theme.toml`.

This spec supersedes the TUI half of "log colors always come from
`alnav::logcolor`". That crate remains the **CLI-only** color source.

---

## Goal

A user sets `theme = "kanagawa"` once, restarts, and the whole TUI — chrome,
Dashboard wordmark, log level badges, timestamps, search highlights, minimap —
reads as that scheme. The current look stays the `default` theme and does not
paint a background.

---

## Decisions (locked)

| ID | Decision |
|----|----------|
| D1 | TUI colors (chrome **and** log rows) follow the selected theme. CLI keeps `alnav-core::logcolor`. |
| D2 | Data model: 18-slot palette is the base; semantic tokens override after mapping (Helix-style). |
| D3 | `default` does not paint a canvas background (`Reset`). Named palettes with a real `background` hex do. Overlay may set `background = "reset"` to disable paint on a named theme, or a hex to enable paint on `default`. |
| D4 | Select with `config.toml` `theme = "<name>"`. `theme.toml` is overlay only. No `--theme` flag. No in-TUI switch. |
| D5 | Overlay `[palette]` keys are flat ANSI 16 + `background` / `foreground`. Not Alacritty nested tables, not Base16 `base00`. |
| D6 | Built-in names (kebab-case): `default`, `onedark`, `dracula`, `everforest`, `tokyo-night`, `catppuccin-mocha`, `gruvbox-dark`, `nord`, `kanagawa`. Variants: Atom One Dark, classic Dracula, Everforest dark-medium, Tokyo Night (Night), Catppuccin Mocha, Gruvbox dark medium, Nord Polar Night, Kanagawa Wave. No light variants. |
| D7 | Highlight ramp of 8 is mapped from palette slots, not hand-tuned per theme. |
| D8 | Theme is applied once at startup via `theme::install`. |

---

## Out of scope

- CLI / `alnav grep` theming
- Runtime theme switch or `--theme`
- Base16 YAML, Alacritty nested `colors.*`, Tinty
- Light variants and extra schemes (Storm, Moon, Macchiato, Dragon, Rosé Pine, …)
- Persisting a TUI choice back into `config.toml`

---

## Architecture

```
config.toml  theme = "kanagawa"     → builtin Palette
theme.toml   [palette] + token keys → optional merge
                    ↓
              Palette (18 slots)
                    ↓
              map_to_tokens()
                    ↓
              theme.toml semantic keys merge
                    ↓
              theme::install (once)
                    ↓
              render only through theme::*
```

`default` is a Palette, not a separate code path. Its chromatic slots are
ratatui named ANSI colors (`Color::Cyan`, …). Its `background` and
`foreground` are `Color::Reset`. Wash backgrounds cannot be mixed from
`Reset`; they keep today's hardcoded values (see Mapping).

`alnav-core::logcolor` is not read by TUI style functions after this change.

---

## Palette

### Slots

`background`, `foreground`,
`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`,
`bright_black`, `bright_red`, `bright_green`, `bright_yellow`,
`bright_blue`, `bright_magenta`, `bright_cyan`, `bright_white`.

Color syntax (unchanged): `#RGB`, `#RRGGBB`, named ANSI (`cyan`, `darkgray`,
`reset`, …).

### Builtin hex sources (pin in `theme_builtins.rs`)

Copy the `primary` + `normal` + `bright` colors from these Alacritty ports.
Do not invent slots. Convert `0xRRGGBB` to `#RRGGBB`.

| Name | Upstream |
|------|----------|
| `onedark` | [alacritty-theme `one_dark.toml`](https://github.com/alacritty/alacritty-theme/blob/master/themes/one_dark.toml) |
| `dracula` | [dracula/alacritty](https://github.com/dracula/alacritty) |
| `everforest` | [sainnhe/everforest](https://github.com/sainnhe/everforest) dark **medium** Alacritty extra |
| `tokyo-night` | [folke/tokyonight.nvim](https://github.com/folke/tokyonight.nvim) **night** (not storm/moon) Alacritty extra |
| `catppuccin-mocha` | [catppuccin/alacritty `catppuccin-mocha.toml`](https://github.com/catppuccin/alacritty/blob/main/catppuccin-mocha.toml) |
| `gruvbox-dark` | [alacritty-theme `gruvbox_dark.toml`](https://github.com/alacritty/alacritty-theme/blob/master/themes/gruvbox_dark.toml) |
| `nord` | [nordtheme/alacritty](https://github.com/nordtheme/alacritty) |
| `kanagawa` | [rebelot/kanagawa.nvim `extras/alacritty_kanagawa.toml`](https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty_kanagawa.toml) (Wave) |

---

## Mapping

Chromatic tokens take a slot. Wash tokens mix RGB. Mix is

```
mix(bg, tint, t) = bg + (tint - bg) * t / 100    // t in 0..=100, per channel, u8
```

On-badge / on-pill foreground: if `0.299R + 0.587G + 0.114B >= 140` then
black, else white.

### Direct slots

| Token | Slot |
|-------|------|
| `accent`, Tag field, `candidate_match_fg` | **signature** (see below); `cyan` on `default` / `nord` |
| `success`, Msg field | `green` |
| `warning`, Level field name | `yellow` |
| `lock`, `selection_frame`, Pid field | `magenta` |
| Tid field | `bright_magenta` |
| Pkg field | `bright_yellow` |
| `error` (minimap severe, crash accent) | `red` |
| `muted`, `border_inactive`, Verbose badge bg | `bright_black` |
| `canvas_fg` | `foreground` |
| `canvas_bg` | `background` (`Reset` ⇒ do not paint) |
| `candidate_selected_fg` | `bright_white` (`White` on `default`) |
| `candidate_unselected_fg` | `white` + DIM on named themes; `Gray` on `default` (today's look) |
| Dashboard wordmark `logo[6]` | per-theme 3-stop ramp (below); solid `cyan` on `default` |

Signature accent (chrome, compact `"alnav"`, Tag, candidate match):

| Theme | Accent slot |
|-------|-------------|
| `default`, `nord` | `cyan` |
| `onedark`, `tokyo-night`, `kanagawa` | `blue` |
| `dracula`, `catppuccin-mocha` | `magenta` |
| `everforest` | `green` |
| `gruvbox-dark` | `yellow` |

Dashboard Unicode wordmark (6 rows) interpolates three palette stops (`mix` at 40/80/50). Compact/Minimal `"alnav"` uses accent. Overlay `accent =` overrides chrome accent only, not the 6-row ramp.

| Theme | Logo stops (top → bottom) |
|-------|---------------------------|
| `default` | solid `cyan` |
| `onedark` | `blue`, `cyan`, `magenta` |
| `dracula` | `cyan`, `magenta`, `blue` |
| `everforest` | `green`, `yellow`, `cyan` |
| `tokyo-night` | `blue`, `magenta`, `cyan` |
| `catppuccin-mocha` | `blue`, `magenta`, `red` |
| `gruvbox-dark` | `yellow`, `red`, `green` |
| `nord` | `blue`, `cyan`, `bright_cyan` |
| `kanagawa` | `blue`, `magenta`, `cyan` |

Dashboard subtitle/footer: `canvas_fg` + DIM on named themes; `muted` + DIM on `default`.

Level badge **backgrounds**: `V=bright_black`, `D=blue`, `I=green`,
`W=yellow`, `E=red`, `F=bright_red` plus bold. Foreground from the luminance
rule.

Highlight ramp (index `i % 8`):
`yellow`, `bright_yellow`, `red`, `magenta`, `blue`, `cyan`, `green`,
`bright_green`. Chip/search text uses the luminance rule on that background.

Log timestamp/pid/tid style uses `muted`. Package tint uses Pkg field color.
CLI `--highlight` / `USER_HIGHLIGHT` stay on `logcolor` (CLI-only).

### Severe log rows (E / F / crash)

When `row.severe` is true (`is_severe_row`: level E/F **or** crash signature),
**tag and message** foreground uses `theme::severe_entry_style(emphatic)`
(`t().error`, default `Color::Red`). Line number and timestamp stay `muted`.
Level badges are unchanged (`level_badge_style`). Keyword / search highlights
still overlay on top.

`emphatic` (Bold) is Fatal (`Level::F`) and crash-signature rows that are not
plain Error. Plain E-level is red without Bold.

Do **not** paint the whole `ListItem` red, and do **not** hard-code `Color::Red`
in `ui.rs`.

Compare-tray Δt / untimed `—` uses `theme::compare_delta_style()` (`muted` + DIM).
Stale origin mark `☆` uses `theme::bookmark_stale_style()`. Do not inline those
colors in `ui.rs`.

### Mix (named themes with RGB `background` only)

| Token | Mix |
|-------|-----|
| `log_selection_bg` | `mix(bg, accent, 22)` |
| `log_visual_bg` | `mix(bg, blue, 26)` |
| `bookmark_row_bg` | `mix(bg, yellow, 12)` |
| `bookmark_strip_bg` | `mix(bg, yellow, 8)` |
| `preview_highlight_bg` | `mix(bg, accent, 28)` |
| `candidate_selected_bg` | `mix(bg, foreground, 16)` |

`candidate_unselected_bg`: `Reset` when `canvas_bg` is `Reset`, else
`canvas_bg`.

### `default` wash exception

When `background` is `Reset`, skip mix and keep current values:

| Token | Value |
|-------|--------|
| `log_selection_bg` | `DarkGray` |
| `log_visual_bg` | `Rgb(30, 60, 70)` |
| `preview_highlight_bg` | `DarkGray` |
| `bookmark_strip_bg` | `DarkGray` |
| `bookmark_row_bg` | `Rgb(54, 46, 0)` |
| `candidate_selected_bg` | `DarkGray` |

`default` chromatic tokens stay named ANSI (`Cyan`, `Green`, `Yellow`,
`Magenta`, `LightYellow`, `LightMagenta`, …) so they still follow the
terminal palette. Mapped equivalent: accent=`Cyan`, success=`Green`, etc.

---

## Config contract

### `config.toml`

```toml
theme = "kanagawa"   # omitted or empty → "default"
```

New field `AppConfig.theme: String`. Unknown keys in `config.toml` stay
ignored (existing behavior).

**Name folding**: trim, lowercase, `_` → `-`. Also accept concatenated
aliases below. Comparison is on the folded kebab form.

| Folded name | Also accepted |
|-------------|----------------|
| `tokyo-night` | `tokyonight`, `tokyo_night` |
| `catppuccin-mocha` | `catppuccin`, `catppuccin_mocha`, `mocha` |
| `gruvbox-dark` | `gruvbox`, `gruvbox_dark` |
| `everforest` | `ever-forest` |
| `onedark` | `one-dark`, `one_dark` |
| `kanagawa` | `kanagawa-wave`, `kanagawa_wave` |
| `dracula` | |
| `nord` | |
| `default` | `builtin` |

`mocha` maps to Catppuccin Mocha because that is the only Catppuccin flavor
in v1. Do not add `latte` / `frappe` as aliases that silently become Mocha.

### `theme.toml` overlay

Existing semantic keys remain valid and merge **after** mapping.
New optional table `[palette]` merges into the selected builtin palette
**before** a second mapping pass.

```toml
accent = "#7aa2f7"

[palette]
red = "#ff5d62"
background = "reset"    # optional: disable canvas paint
```

Optional `highlight = ["#...", ...]` — must be length 8. Replaces the mapped
ramp after mapping.

Unknown keys are ignored (forward compatible), except a present `highlight`
with length ≠ 8, which is a parse error.

`candidate_prefix` stays a string overlay, not a color.

### Load order

1. Read `theme` from config (missing/empty → `default`).
2. Resolve builtin Palette (unknown name → `default` + Fallback status).
3. If `theme.toml` exists, parse it. On any parse error (bad TOML, illegal
   color, `highlight` length ≠ 8): keep step 2 tokens, Fallback status,
   **do not** apply a partial overlay.
4. Else merge `[palette]` onto the builtin Palette, `map_to_tokens` again,
   then merge semantic keys (including `highlight` if present).
5. `theme::install`.

If both the theme name is unknown **and** overlay parse fails, prefer the
overlay error in the status hint (it is the file the user just edited).
Unknown name with a **valid** overlay: default palette + overlay, status
still reports the unknown name.

Missing `theme.toml`: silent, builtin only.

CLI does not read `theme` or `theme.toml`.

---

## Code layout

Flat modules under `alnav/src/`, matching [directory-structure.md](./directory-structure.md):

| File | Role |
|------|------|
| `theme.rs` | Glyphs + `*_style()` / `*_color()`. Only module `ui.rs` calls for paint. Reads installed tokens. **No** `Color::*` literals except inside token construction helpers used by the mapper. |
| `palette.rs` | `Palette`, name fold, `map_to_tokens`, `mix`, luminance. |
| `theme_builtins.rs` | Nine palettes (`default` + eight named) as `Palette` constants. |
| `config.rs` | `AppConfig.theme`; `load_theme(dir, name)` implements the load order. |

Startup: `load_config` then `load_theme(dir, &cfg.theme)`.

Canvas paint: root layout / LogList / strips use `theme::canvas_bg()` as
widget background when it is not `Reset`. Do not OSC-set the terminal
background.

Implementation must update:

- [directory-structure.md](./directory-structure.md) — add `palette.rs` /
  `theme_builtins.rs`; `theme.rs` no longer derives TUI log color from
  `logcolor`.
- [quality-guidelines.md](./quality-guidelines.md) — TUI log colors come
  from installed tokens; CLI still uses `logcolor`.
- `alnav/examples/theme.toml` and `alnav/examples/config.toml` (commented
  `theme =`).
- CLAUDE.md / AGENTS.md theme bullets to match D1–D8.

---

## Testing

| Case | Expect |
|------|--------|
| Missing `theme` key | `default`; `accent == Color::Cyan`; `canvas_bg == Reset` |
| `theme = "TokyoNight"` / `tokyo_night` | same tokens as `tokyo-night` |
| `theme = "mocha"` | `catppuccin-mocha` |
| `theme = "not-a-theme"` | `default` + Fallback hint contains `unknown theme` |
| Overlay `[palette] red = "#ff0000"` only | `error` / level E bg is that red; `accent` still the builtin cyan |
| Overlay `accent = "#ffffff"` | accent white; palette-derived success unchanged |
| Overlay `background = "reset"` on `kanagawa` | `canvas_bg == Reset` |
| Overlay `highlight` length 7 | entire overlay ignored; builtin tokens; Fallback |
| Bad TOML in `theme.toml` | builtin tokens; Fallback; no panic |
| `kanagawa` | `canvas_bg` is RGB `#1f1f28`; highlight[0] equals that palette's `yellow` |
| CLI parser/formatter tests | unchanged colors from `logcolor` |

No TUI screenshot tests required. Visual check: launch `-f` with each of
the nine names once during implementation QA.

---

## Acceptance

- [ ] `config.toml` `theme` selects a builtin; omitted → `default`.
- [ ] `default` matches today's chrome (Cyan accent, Reset canvas, existing wash RGB/DarkGray).
- [ ] Each named theme paints canvas from its `background` and recolors log badges, muted timestamp, and 8-step highlight from the mapping table.
- [ ] `theme.toml` can override palette slots and/or semantic tokens; parse errors fall back without a mixed token set.
- [ ] `ui.rs` (non-test) has no `Color::*` literals.
- [ ] `alnav grep` colored output is bit-for-bit the pre-change `logcolor` path.
- [ ] Unknown theme name and bad overlay are visible on the status bar the same way today's bad `theme.toml` is.
- [ ] Examples and the three docs listed under Code layout are updated.

---

## Risks

- Mix percentages may under-contrast on Nord / Everforest. Adjusting a
  percentage inside 8..=25 during QA is allowed; changing the mix **inputs**
  (which slot tints selection) needs a spec amendment.
- Named ANSI on `default` vs hex on named themes means `Color::Cyan !=`
  Tokyo Night cyan. That is intended.
- TUI vs CLI color divergence is intended (D1).
