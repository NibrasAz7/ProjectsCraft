# ProjectsCraft — Vision

An open creative suite, rebuilt from scratch in Rust — and a playground
that proves the whole suite can produce real work end to end.

## The idea

Every professional creative discipline has a dominant proprietary tool.
ProjectsCraft gathers the family of clean-room, open-source
reimplementations of those tools — the **Craft apps** — and the shared
infrastructure that makes them feel like one suite rather than a dozen
separate programs.

Each app is native, offline, memory-safe, and written entirely in Rust.
Each reads and writes the file formats its proprietary counterpart made
the industry standard. Each is built so that an AI agent can drive it
as fluently as a human can click it.

## The suite

### The creative tools (Adobe-class)

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

### The office tools (Microsoft-class)

| App | Discipline | Reference |
| --- | --- | --- |
| **WordCraft** | Writing and document design, `.docx` | Word |
| **GridCraft** | Spreadsheets — formulas, charts, `.xlsx` | Excel |
| **DeckCraft** | Presentations — slides, charts, `.pptx` | PowerPoint |

### The technical tools

| App | Discipline | Reference |
| --- | --- | --- |
| **MatCraft** | Numerical computing, `.m`-language engine | MATLAB |
| **TexCraft** | LaTeX authoring and typesetting, live PDF | Overleaf-class |
| **ScreenCraft** | Screen recording with automatic zooms and captions | Screen Studio |

### The platform underneath

| Repo | Role |
| --- | --- |
| **craft-launcher** | Installs, updates and launches the suite — native egui desktop app, same Rust compiled to WASM for the web |
| **craft-libs** | Shared Rust libraries (atomic file I/O, ICC colour management, printing…) used by every app |
| **craft-fonts** | The suite's font assets, kept out of the app repos |

## The principles

The apps are different; the way they are built is the same. That is the
point of the suite.

- **Clean-room.** Behaviour and look are studied, never copied.
  Implementations come from public specifications, published algorithms
  and papers — PSD spec, ICC, ISO 32000, Knuth–Plass. No proprietary
  code, assets, profiles or GPL-licensed alternatives.
- **Rust only.** No Tauri, Electron or webview shells. The desktop app is
  native egui/eframe; the web build is the same Rust compiled to
  WebAssembly. Memory safety and fearless concurrency are free.
- **Never crash.** A corrupt file, a hostile input, a full disk must
  produce an actionable error, never a panic. Enforced by clippy lints:
  no `unwrap`, `expect`, `panic` or `unsafe` in non-test code.
- **Everything is a command.** Every user-visible action is a command in
  the engine — reachable identically from menus, the headless CLI, the
  TCP control channel and MCP. The UI is thin; agents are first-class
  users, not an afterthought.
- **Layered architecture.** A shared crate skeleton — geometry and colour
  at L0, document model and engines in the middle, UI at the top —
  enforced mechanically by `cargo xtask layers`.
- **Native files, real exports.** Each app keeps its own project format
  *and* reads/writes the industry's interchange formats, so work is
  never trapped and never needs a subscription to reopen.
- **Runs everywhere.** macOS, Windows, Linux, FreeBSD natively; the
  browser via WASM.

## The proof: a whole campaign from the CLIs

**PlaygroundCraft** is the working demonstration: a complete
product-launch campaign for **LUMA AURA** (adaptive ambient lighting,
*See light differently*) — twelve projects, one per creative app, all
produced by driving the command-line interfaces:

brand identity → hero poster → product photo library → brochure →
press release → investor deck → launch budget → trade-show booth plan →
promo track → promo video → logo sting → combined press-kit PDF.

Every project is a self-contained directory with a `build.py` that
reproduces its native project file and its publication exports. It is
both a test of the tools and a template for what a connected open suite
makes possible.

## Where this is going

- **Parity, then beyond.** Each app tracks a measured parity score
  against its reference (menus, files, behaviour, performance). The near
  goal is 1:1 coverage; the real goal is better — faster, safer, scriptable.
- **One agent vocabulary.** Because every action in every app is a
  command, a single agent can plan a deliverable and execute it across
  raster, vector, layout, motion, sound, documents, sheets, CAD, math
  and typesetting — the whole pipeline, no human handoffs required.
- **A platform others can build on.** `craft-libs` crates are designed to
  outgrow the suite; anything broadly useful (RAW support, colour
  management, printing) can be promoted into its own library for the
  wider ecosystem.
- **Open by default.** MIT OR Apache-2.0 code, licensed assets with full
  attribution, reproducible builds, public roadmaps.

## The name

*ProjectsCraft* is the umbrella: the collection of the Craft apps, the
shared platform beneath them, and the playground that shows what they
can do together.
