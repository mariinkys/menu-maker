<div align="center">
<br>
<img src="./resources/icons/hicolor/scalable/apps/icon.svg" width="150" />
<h1 align="center">MenuMaker</h1>

![GitHub License](https://img.shields.io/github/license/mariinkys/menu-maker)

<h3>Desktop app for designing single-page restaurant menus</h3>

<img src="./resources/screenshots/main.png" width=700>

</div>

<br>

Desktop app made with [Slint](https://slint.dev) for designing the restaurant menu of
Hotel Casa Peix (Customizable to other restaurants).

- Customizable header with logo: left, centre or right, with adjustable height.
- Sections with dishes: name, description, price and allergens.
- Layout: 1, 2 or 3 columns, aligned left (price column with dot leaders), centre or right.
- Per-role fonts (title, subtitle, sections, dishes, description, price) chosen from the
  system fonts, with size, bold and italic. Also colours, margins, paper size (A4 by default,
  A5, A3, Letter, Legal or custom) and orientation.
- **Always one sheet.** A binary search over a global text scale shrinks the menu until it fits
  and, optionally, enlarges it to fill the page. Columns are balanced, a section title never
  ends up alone at the bottom of a column, and sections are kept together when possible.
- Live preview: the preview is the real PDF rasterised with hayro, so what you see is exactly
  what you export.
- PDF export with [krilla](https://github.com/LaurenzV/krilla) (fonts embedded and subset).
- Presets in SQLite: full menu or design only. Plus autosave and JSON import/export.
- **Not tied to one restaurant.** Customizable app and menu designs.

## Architecture

```
src/
├── lib.rs                 menu_core: all logic, with no UI dependency
│   ├── domain/            pure data model (menu, sections, dishes, allergens, style, prices)
│   ├── fonts.rs           system fonts (fontdb), metrics/coverage (skrifa), with fallbacks
│   ├── layout/            layout engine
│   │   ├── text.rs        shaping (harfrust) + cache + line breaking
│   │   ├── blocks.rs      header, section titles, dishes, legend → positioned blocks
│   │   ├── flow.rs        column distribution and balancing
│   │   ├── model.rs       DrawOp: text, lines, circles, logo (points, origin top-left)
│   │   └── mod.rs         fit search → PageLayout
│   ├── render/            pdf.rs (krilla) and raster.rs (hayro)
│   ├── engine.rs          facade: MenuDocument → PDF / preview bitmap
│   ├── storage.rs         SQLite (rusqlite, bundled): presets, autosave, app settings
│   ├── settings.rs        app settings (theme, colour, title, logo) + contrast helpers
│   └── paths.rs           data folder via `dirs` 
├── main.rs                binary `menu-maker`: logging + Slint UI
├── logging.rs             tracing setup (stderr + rolling log file)
└── app/                   UI controller
    ├── mod.rs             connects the Actions callbacks to the core
    ├── editor.rs          document + view state (expanded rows)
    ├── convert.rs         domain ↔ Slint types
    ├── branding.rs        applies app settings to Theme/Branding, reads the settings dialog
    └── preview.rs         background thread with debounce for the preview
ui/                        .slint files (interface only; no logic)
```

Data flows one way: the UI calls `Actions.*`. The controller updates the `MenuDocument`,
then `MenuEngine` lays it out (`layout`) and writes the PDF (`render`). The same text shaping
is used for measuring and for drawing, so positions in the PDF match the layout exactly.

## Building

```bash
# Linux (Debian/Ubuntu): fontconfig headers
sudo apt install libfontconfig1-dev

cargo run --release             # binary: target/release/menu-maker
cargo test                      # 29 tests: prices, layout, flow, PDF, SQLite, logos, contrast…
```

On Windows and macOS no extra system dependencies are needed.

## AI Disclosure

No agentic AI (vibe-coding) has been used for the development of this app; however, I've used the web interface extensively for asking questions and writing some parts of the code, specially regarding the PDF rendering. This is the first time I use Slint, so some questions regarding the architecture of the app have also been solved largely by AI.

The model I've used is Opus 5.5 Medium Effort.
