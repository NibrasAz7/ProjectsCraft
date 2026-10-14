# ProjectsCraft — Vision

Real projects, built entirely with the ArtCraft suite — proving that an
open, Rust-native creative toolchain can carry a complete professional
pipeline.

> **Attribution.** The Craft apps and the ArtCraft suite are developed by
> **[storytold](https://github.com/storytold)** — the official source.
> ProjectsCraft is an independent companion effort: it uses, exercises
> and contributes back to the suite; it does not develop the apps
> themselves.

## The idea

The ArtCraft suite reimplements the industry's dominant creative, office
and technical tools as clean-room, open-source, Rust-native apps —
offline, memory-safe, and designed to be driven by agents as fluently
as by humans.

A suite claim like that needs evidence, not just repos. **ProjectsCraft
is the evidence layer**: a series of real, reproducible projects that
drive the tools end to end through their CLIs — the way a professional
studio would use them, and the way an autonomous agent would.

## The suite being exercised

### Creative tools (Adobe-class)

| App | Discipline | Reference |
| --- | --- | --- |
| **PhotoCraft** | Image editing — layers, masks, adjustments, real PSD | Photoshop |
| **VectorCraft** | Vector illustration — paths, type, `.ai` | Illustrator |
| **DesignCraft** | Page layout and publishing | InDesign |
| **LightCraft** | Photo library and raw development | Lightroom |
| **FilmCraft** | Video editing, colour, sound | Premiere Pro |
| **EffectCraft** | Motion graphics and visual effects | After Effects |
| **SoundCraft** | Audio recording, editing, mixing | Pro Tools |
| **PdfCraft** | The PDF workbench — combine, bookmark, preflight | Acrobat |
| **CADCraft** | Computer-aided drafting, `.dxf` | AutoCAD |

### Office tools (Microsoft-class)

| App | Discipline | Reference |
| --- | --- | --- |
| **WordCraft** | Writing and document design, `.docx` | Word |
| **GridCraft** | Spreadsheets — formulas, charts, `.xlsx` | Excel |
| **DeckCraft** | Presentations — slides, charts, `.pptx` | PowerPoint |

### Technical tools

| App | Discipline | Reference |
| --- | --- | --- |
| **MatCraft** | Numerical computing, `.m`-language engine | MATLAB |
| **TexCraft** | LaTeX authoring and typesetting, live PDF | Overleaf-class |
| **ScreenCraft** | Screen recording with automatic zooms and captions | Screen Studio |

### Platform

| Repo | Role |
| --- | --- |
| **craft-launcher** | Installs, updates and launches the suite — native egui desktop app, same Rust compiled to WASM for the web |
| **craft-libs** | Shared Rust libraries (atomic file I/O, ICC colour management, printing…) used by every app |
| **craft-fonts** | The suite's font assets |

## Why a projects repo

- **Dogfooding at suite scale.** Each app's repo proves the app;
  ProjectsCraft proves the *suite* — that assets, formats and intent
  flow across tools the way a real production pipeline needs.
- **Reproducible by construction.** Every project is a self-contained
  directory with a `build.py` that regenerates its native project file
  and publication exports from the CLIs alone.
- **Agent-first.** Because every action in every Craft app is a command
  (engine → CLI → control channel → MCP), these projects double as a
  benchmark for how far an agent can drive the whole suite unaided.
- **A learning trail.** Captured command catalogs and per-tool notes
  show what works, what's missing, and what to contribute upstream.

## Campaign 01: LUMA AURA

The first exercise: a complete product-launch campaign for **LUMA AURA**
— adaptive ambient lighting, *See light differently* — twelve projects,
one per creative tool:

brand identity → hero poster → product photo library → brochure →
press release → investor deck → launch budget → trade-show booth plan →
promo track → promo video → logo sting → combined press-kit PDF.

Every deliverable built headlessly from the command-line interfaces.

## Contribution loop

Work here feeds back into the suite: gaps found during real projects
become issues, fixes and features land through the forks under
`NibrasAz7`, and reusable learnings go to the shared `craftrules`
standards. Localization work (full Arabic/RTL UI) is developed the same
way — proven in projects first, contributed upstream second.

## Where this is going

- **More campaigns.** One domain per exercise: scientific publishing
  (TexCraft + MatCraft + GridCraft), broadcast package, conference
  booth-to-video pipeline, documentation sets.
- **Deeper agent coverage.** Each campaign pushes the boundary of what a
  single agent can ship without human handoffs.
- **A public record.** Open deliverables, open build scripts, open notes
  — a template others can copy to validate or extend the suite.
