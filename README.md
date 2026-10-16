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

**Alpha (M0–M6 landed)** — desktop Studio (`projectcraft`), headless CLI
(`projectcraft-cli`), self-hosted collaboration relay (`projectcraft-sync`),
WebAssembly browser shell (`projectcraft-web`), MCP stdio server, TCP
JSON-lines control channel, and Windows WiX v5 MSI + portable zip packaging
are in place.

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
- **Collaboration & audit DAG (`pmsync`, `projectcraft-sync`):** role-based
  permissions (`Owner`, `Editor`, `Commenter`, `Viewer`, `Guest`),
  content-addressed change envelopes, `@mention` task comments, idempotent
  peer bundle merging, post-merge semantic schedule conflict advisories, and
  a single-binary JSON-lines relay server (`projectcraft-sync`).
- **Command Engine, MCP, Studio & Web (`engine`, `mcp`, `ui-egui`, `apps/*`):**
  38 panic-guarded engine commands reachable identically from the Studio UI,
  `projectcraft-cli --cmd`, the TCP control server (`--control PORT`), and
  `projectcraft-cli mcp`; `projectcraft-web` compiles cleanly to
  `wasm32-unknown-unknown` (`cargo xtask wasm`).

**Honest limitations (not yet implemented):**

- Binary `.mpp` OLE container parsing is not yet implemented (use Microsoft
  Project MSPDI `.xml` or Primavera `.xer` for schedule interchange).
- Windows installer packages (`packaging/windows/package.ps1` → WiX v5 `.msi`
  and portable `.zip`) build unsigned unless code-signing credentials are
  supplied; macOS and Linux installer bundles are planned.

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
cargo xtask ci
```

## Workspace layout

| Layer | Path | Role |
| --- | --- | --- |
| L0 | `crates/pmcore` | Project document model, WBS, work items, calendars, resources, sprints, baselines |
| L1 | `crates/pmsched` | CPM forward/backward pass, levelling, baseline variance, EVM, Monte Carlo risk |
| L2 | `crates/pmformat` | `.projectcraft`, CSV, Jira JSON, MSPDI `.xml`, Primavera `.xer`, SVG Gantt & HTML report |
| L3 | `crates/pmsync` | Role permissions, content-addressed change DAG, `@mention` comments, post-merge conflict detector |
| L5 | `crates/engine` | 38-command registry + panic-safe `Session` with undo/redo |
| L6 | `crates/mcp` | JSON-RPC 2.0 MCP server over stdio |
| L6 | `crates/ui-egui` | Desktop Studio UI (Table, Gantt, Board, Resources, EVM & Risk) + TCP control dispatch |
| L7 | `apps/projectcraft-cli` | Headless CLI (`demo`, `run`, `schedule`, `evm`, `risk`, `commands`, `mcp`, `--cmd`) |
| L7 | `apps/projectcraft` | Desktop Studio (`projectcraft [--control PORT] [file]`) |
| L7 | `apps/projectcraft-sync` | Self-hosted JSON-lines collaboration relay (`projectcraft-sync [--bind ADDR] [--store FILE]`) |
| L7 | `apps/projectcraft-web` | Browser WASM entry (`eframe::WebRunner`, verified by `cargo xtask wasm`) |
| — | `packaging/windows` | WiX v5 `.wxs`, `package.ps1`, and `sign.ps1` for `.msi` + portable `.zip` releases |
| — | `xtask` | `cargo xtask ci` (`fmt`, `clippy -D warnings`, `test`, `layers`, `wasm`) |

## License

MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
