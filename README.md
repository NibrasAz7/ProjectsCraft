# ProjectCraft

A clean-room, open-source, Rust-native project-management suite — the
[ArtCraft](https://github.com/storytold)-style alternative to **Jira,
monday.com, Asana, ClickUp and Microsoft Project** in a single local-first
tool.

One engine, five lenses over the same document: WBS tables, interactive
Gantt charts with critical-path scheduling, WIP-limited Kanban and Scrum
boards, resource workload heatmaps with priority-rule levelling, and
ANSI/EIA-748 Earned Value Management (`PV`, `EV`, `AC`, `SV`, `CV`, `SPI`,
`CPI`, `EAC`) with Monte Carlo schedule-risk simulation (`P10`/`P50`/`P80`/`P90`).

## Status

**Alpha** — working desktop Studio (`projectcraft`), headless CLI
(`projectcraft-cli`), MCP stdio server, TCP JSON-lines control channel,
and core scheduling/agile/resource/EVM/interop crates are in place.

**What works today:**

- **Core project model (`pmcore`):** WBS hierarchy, work items (`task`,
  `milestone`, `phase`, `epic`, `story`, `bug`), custom fields, workflow
  statuses (`backlog`, `todo`, `in_progress`, `blocked`, `review`, `done`),
  priorities, dependencies (`FS`, `SS`, `FF`, `SF` with positive lag and
  negative lead), task constraints (`ASAP`, `ALAP`, `SNET`, `SNLT`, `FNET`,
  `FNLT`, `MSO`, `MFO`), calendars, resources with effective-dated rate
  tables, agile sprints, and multiple named baselines.
- **Scheduling, Resource Levelling, EVM & Risk (`pmsched`):** topological
  CPM forward/backward pass with cycle detection, total and free slack,
  critical path identification, baseline-vs-actual variance, daily resource
  workload heatmaps, priority-rule resource levelling with human-readable
  explanations, ANSI/EIA-748 Earned Value Management (`BAC`, `PV`, `EV`,
  `AC`, `SV`, `CV`, `SPI`, `CPI`, `EAC`, `ETC`, `VAC`, `TCPI`, S-curves),
  sprint burndown, and deterministic three-point PERT Monte Carlo
  schedule-risk simulation (`P10`/`P50`/`P80`/`P90` and per-task criticality
  index).
- **Interop & headless rendering (`pmformat`):** native `.projectcraft`
  UTF-8 JSON, RFC-4180 CSV, Jira issue JSON, Microsoft Project MSPDI `.xml`,
  and Primavera P6 `.xer` import/export, plus headless SVG Gantt chart and
  standalone HTML executive report generation.
- **Command Engine, MCP & Studio (`engine`, `mcp`, `ui-egui`, `apps/*`):**
  33 panic-guarded engine commands reachable identically from the Studio UI,
  `projectcraft-cli --cmd`, the TCP control server (`--control PORT`), and
  `projectcraft-cli mcp`.

**Honest limitations (not yet implemented):**

- Binary `.mpp` OLE container parsing is not yet implemented (use Microsoft
  Project MSPDI `.xml` or Primavera `.xer` for schedule interchange).
- Real-time multiplayer CRDT sync (`Automerge` relay) and WASM web packaging
  are planned for M6.

Read **[vision.md](vision.md)** for the product vision,
**[ROADMAP.md](ROADMAP.md)** for milestone status,
**[docs/architecture.md](docs/architecture.md)** for the crate layering, and
**[docs/gaps.md](docs/gaps.md)** for known compatibility gaps.

## Quick start

```bash
cargo run -p projectcraft-cli -- commands
cargo run -p projectcraft-cli -- demo
cargo run -p projectcraft-cli -- schedule
cargo run -p projectcraft-cli -- evm
cargo run -p projectcraft-cli -- risk
cargo run -p projectcraft
```

## Workspace layout

| Layer | Path | Role |
| --- | --- | --- |
| L0 | `crates/pmcore` | Project document model, WBS, work items, calendars, resources, sprints, baselines |
| L1 | `crates/pmsched` | CPM forward/backward pass, levelling, baseline variance, EVM, Monte Carlo risk |
| L2 | `crates/pmformat` | `.projectcraft`, CSV, Jira JSON, MSPDI `.xml`, Primavera `.xer`, SVG Gantt & HTML report |
| L5 | `crates/engine` | 33-command registry + panic-safe `Session` with undo/redo |
| L6 | `crates/mcp` | JSON-RPC 2.0 MCP server over stdio |
| L6 | `crates/ui-egui` | Desktop Studio UI (Table, Gantt, Board, Resources, EVM & Risk) + TCP control dispatch |
| L7 | `apps/projectcraft-cli` | Headless CLI (`demo`, `run`, `schedule`, `evm`, `risk`, `commands`, `mcp`, `--cmd`) |
| L7 | `apps/projectcraft` | Desktop Studio (`projectcraft [--control PORT] [file]`) |
| — | `xtask` | `cargo xtask ci` (`fmt`, `clippy -D warnings`, `test`, `layers`) |

## License

MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
