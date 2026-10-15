# ProjectCraft — instructions for agents

ProjectCraft is a clean-room, open-source, Rust-native project-management
suite — the ArtCraft ecosystem's answer to Jira, monday.com, Asana, ClickUp
and Microsoft Project in a single local-first tool. It runs natively on
Windows, macOS, Linux and BSD, and targets WebAssembly for the web.

## Start every session here

1. Read [`ROADMAP.md`](ROADMAP.md) (milestone, current status, what's next).
2. Read [`vision.md`](vision.md) and [`README.md`](README.md) for product
   scope and architectural commitments.
3. Behaviour reference: public specifications and published scheduling
   algorithms only (see Clean-room below).

## Non-negotiables

- **Clean-room: never copy proprietary code or assets.**
  - Behaviour comes only from public documentation, published scheduling
    and operations-research literature (Kelley & Walker CPM, PERT
    three-point estimation, serial/parallel SGS resource levelling,
    ANSI/EIA-748 Earned Value Management), public interchange schemas
    (MSPDI XML, Primavera XER tabular layout, Jira JSON/CSV), and user-owned
    project files.
  - Never copy GPL/LGPL code or any code whose license conflicts with
    `MIT OR Apache-2.0`.
- **Never crash: no panics in non-test code.** Project files, CSV/XML/XER
  imports, CLI arguments, control messages and MCP requests are untrusted
  input. Every fallible operation returns `Result<T, E>`; cap input-derived
  allocations and loop iterations; index slices with `.get()` rather than
  `[]`. Workspace lints (`clippy::unwrap_used`, `clippy::expect_used`,
  `clippy::panic`, `clippy::unimplemented`, `clippy::todo`,
  `clippy::unreachable`) and `unsafe_code = "forbid"` enforce this.
- **Rust only.** No handwritten JS/TS; never break a future `wasm32` target
  (do not call `Instant::now` or `SystemTime::now` on `wasm32`).
- **Layering** (enforced by `cargo xtask layers`):
  - `pmcore` (L0): core project document model, WBS, work items, custom
    fields, dependencies, constraints, calendars, resources, rate tables,
    sprints, boards, baselines.
  - `pmsched` (L1): CPM forward/backward pass, critical path, slack,
    calendar math, baseline variance, resource workload & priority-rule
    levelling, Earned Value Management (PV/EV/AC, SV/CV, SPI/CPI, EAC),
    and background-ready Monte Carlo schedule-risk simulation.
  - `pmformat` (L2): `.projectcraft` native JSON, CSV, Jira JSON, MSPDI XML,
    Primavera P6 `.xer` import/export, headless SVG Gantt renderer and HTML
    report generator.
  - `engine` (L5): command registry + panic-guarded `Session::execute`.
  - `mcp`, `ui-egui` (L6): JSON-RPC 2.0 stdio MCP server; egui Studio UI
    with Table, Gantt, Kanban Board, Resource Heatmap, and EVM/Risk views.
  - `apps/*` (L7): `projectcraft-cli` and `projectcraft` desktop Studio +
    TCP JSON-lines control channel.
  - Nothing below L6 may depend on `egui`, `eframe`, `winit`, `wgpu`, or `rfd`.
- **Everything is a command:** every user-visible mutation or query has a
  stable command id (`task.add`, `dep.add`, `schedule.run`,
  `resource.level`, `evm.compute`, …) reachable identically from the Studio
  UI, `projectcraft-cli --cmd`, the TCP control channel, and MCP.
- **Quality gates** before every commit: `cargo xtask ci` (`fmt`, `clippy -D
  warnings`, `test`, `layers`).
