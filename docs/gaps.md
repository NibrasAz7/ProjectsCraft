# ProjectCraft — known gaps and honest compatibility status

This document tracks what is implemented today versus what remains open on
the roadmap (`ROADMAP.md`), so users and catalog reviewers have an accurate
picture of compatibility.

## Implemented today (Alpha)

- **Scheduling (`pmsched`):** CPM forward/backward pass, `FS`/`SS`/`FF`/`SF`
  dependencies with positive lag and negative lead, task constraints (`ASAP`,
  `ALAP`, `SNET`, `SNLT`, `FNET`, `FNLT`, `MSO`, `MFO`), total and free slack,
  critical path identification, and cycle detection.
- **Resources & Levelling (`pmsched`):** resource pool with max units,
  effective-dated rate tables, time-phased daily workload heatmaps,
  overallocation detection, and serial priority-rule heuristic levelling with
  per-shift explanations.
- **Earned Value & Risk (`pmsched`):** multiple named baselines, per-task
  schedule and cost variance, ANSI/EIA-748 EVM (`BAC`, `PV`, `EV`, `AC`, `SV`,
  `CV`, `SPI`, `CPI`, `EAC`, `ETC`, `VAC`, `TCPI`, cumulative S-curves), and
  three-point PERT Monte Carlo simulation (`P10`, `P50`, `P80`, `P90` and
  criticality index).
- **Agile (`pmcore` + `pmsched` + `ui-egui`):** backlogs, sprints, story
  points, WIP-limited Kanban board, and sprint burndown.
- **Interop (`pmformat`):** `.projectcraft` JSON, RFC-4180 CSV, Jira JSON,
  Microsoft Project MSPDI `.xml`, Primavera P6 `.xer`, headless SVG Gantt
  export, and standalone HTML executive reports.

## Open gaps (planned for M6+)

| Id | Area | Gap & current workaround |
| --- | --- | --- |
| G1 | Binary `.mpp` files | Microsoft Project binary OLE2 `.mpp` containers are not parsed directly; export/import MSPDI `.xml` or Primavera `.xer` instead. |
| G2 | Real-time multiplayer sync | Projects are local-first `.projectcraft` files today; the Automerge CRDT + WebSocket relay server (`projectcraft-sync`) is planned for M6. |
| G3 | Sub-day calendar exceptions | Task durations and lags are expressed in working days (`f64`) over a 7-day working-weekday mask plus holiday offsets; shift-level hourly calendars are planned. |
| G4 | Automation rules engine | Trigger–condition–action rules ("when status → done, notify owner") and outbound webhooks are planned. |
| G5 | Web (`wasm32`) bundle & OS installers | The crate graph is kept `wasm32`-clean (`clippy.toml` forbids `Instant::now`/`SystemTime::now`); `apps/projectcraft-web` and signed WiX MSI/macOS/Linux installers are planned. |
