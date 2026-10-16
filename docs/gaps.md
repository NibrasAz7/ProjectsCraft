# ProjectCraft — known gaps and honest compatibility status

This document tracks what is implemented today versus what remains open, so
users and catalog reviewers have an accurate picture of compatibility.

## Implemented today (Alpha — M0–M6)

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
- **Collaboration, Web & Packaging (`pmsync`, `projectcraft-sync`,
  `projectcraft-web`, `packaging/windows/`):** role-based access control
  (`Owner`/`Editor`/`Commenter`/`Viewer`/`Guest`), content-addressed change
  DAG, `@mention` comments, post-merge semantic schedule conflict advisories,
  `wasm32-unknown-unknown` browser target (`cargo xtask wasm`), and Windows
  WiX v5 `.msi` + portable `.zip` packaging.

## Open gaps

| Id | Area | Gap & current workaround |
| --- | --- | --- |
| G1 | Binary `.mpp` files | Microsoft Project binary OLE2 `.mpp` containers are not parsed directly; export/import MSPDI `.xml` or Primavera `.xer` instead. |
| G2 | Sub-day calendar exceptions | Task durations and lags are expressed in working days (`f64`) over a 7-day working-weekday mask plus holiday offsets; shift-level hourly calendars are planned. |
| G3 | Automation rules & webhooks | Trigger–condition–action rules ("when status → done, notify owner") and outbound HTTP webhooks are planned. |
| G4 | macOS / Linux installers & Windows code signing | `packaging/windows/package.ps1` produces `.msi` and portable `.zip` bundles (unsigned unless `WINDOWS_CERTIFICATE` or `AZURE_*` secrets are configured); macOS `.dmg` and Linux `.deb`/`.rpm` packaging are planned. |
