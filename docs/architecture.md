# ProjectCraft architecture

ProjectCraft is organized in strict layers enforced by `cargo xtask layers`.
No crate may depend on a crate at the same or higher layer, and nothing
below `L6` may depend on UI or windowing crates (`egui`, `eframe`, `winit`,
`wgpu`, `rfd`). Every crate from `L0` through `L6` plus `apps/projectcraft-web`
is verified against `wasm32-unknown-unknown` via `cargo xtask wasm`.

## Layer table

| Layer | Crate | Responsibility |
| --- | --- | --- |
| L0 | `crates/pmcore` | Unified project document (`Project`): WBS hierarchy, work items (`Task`), custom fields, workflow statuses, priorities, dependencies (`FS`/`SS`/`FF`/`SF` + leads/lags), task constraints (`ASAP`/`ALAP`/`SNET`/`SNLT`/`FNET`/`FNLT`/`MSO`/`MFO`), calendars, resources, effective-dated rate tables, agile sprints, and named baselines |
| L1 | `crates/pmsched` | Topological sort & cycle detection, CPM forward/backward pass (`ES`/`EF`/`LS`/`LF`, total/free slack, critical path), baseline variance, daily resource workload heatmaps, priority-rule resource levelling with explanations, ANSI/EIA-748 Earned Value Management (`BAC`, `PV`, `EV`, `AC`, `SV`, `CV`, `SPI`, `CPI`, `EAC`, `ETC`, `VAC`, `TCPI`, S-curves), sprint burndown, and deterministic `xorshift64*` Monte Carlo schedule-risk simulation |
| L2 | `crates/pmformat` | Native `.projectcraft` JSON, RFC-4180 CSV, Jira issue JSON, Microsoft Project MSPDI `.xml`, and Primavera P6 `.xer` import/export; headless SVG Gantt renderer and standalone HTML executive report generator |
| L3 | `crates/pmsync` | Role-based permissions (`Owner`, `Editor`, `Commenter`, `Viewer`, `Guest`), content-addressed FNV-1a change envelopes (`ChangeEnvelope`), `@mention` task comments, idempotent peer merge, and post-merge semantic schedule conflict advisories |
| L5 | `crates/engine` | 38-command registry (`COMMANDS`) and panic-guarded `Session::execute` with automatic rollback on panic and bounded undo/redo stack |
| L6 | `crates/mcp` | JSON-RPC 2.0 Model Context Protocol (MCP) server over stdio (`2025-06-18`) |
| L6 | `crates/ui-egui` | Desktop Studio (`egui`/`eframe`) with 5 interactive lenses (Table & WBS, Gantt & CPM, Agile Board, Resources & Levelling, EVM & Monte Carlo), Task Inspector, command bar, and TCP control dispatch |
| L7 | `apps/projectcraft-cli` | Headless CLI (`demo`, `run`, `schedule`, `evm`, `risk`, `commands`, `mcp`, `--cmd`) |
| L7 | `apps/projectcraft` | Desktop Studio binary (`projectcraft [--control PORT] [file]`) |
| L7 | `apps/projectcraft-sync` | Single-binary JSON-lines collaboration relay (`projectcraft-sync [--bind ADDR] [--store FILE]`) |
| L7 | `apps/projectcraft-web` | Browser WebAssembly shell (`eframe::WebRunner`) |

## Key invariants

1. **One document, every lens.** A `pmcore::Task` is simultaneously a WBS row,
   a Gantt bar, a Kanban card, and an EVM cost account. Editing a task in any
   view mutates the single `Project` via `Session::execute` and immediately
   reschedules the dependency graph.
2. **Everything is a command.** UI controls, `projectcraft-cli --cmd`, the
   TCP JSON-lines control channel, `projectcraft-sync`, and the MCP server all
   dispatch through `Session::execute(id, params)`.
3. **Never crash on untrusted input.** File parsers cap input bytes
   (`MAX_INPUT_BYTES = 32 MiB`) and task counts (`MAX_TASKS = 100,000`),
   CPM rejects dependency cycles via `SchedError::CycleDetected`, resource
   levelling bounds passes, and `Session::execute` catches unwinds and
   restores the pre-call snapshot.
