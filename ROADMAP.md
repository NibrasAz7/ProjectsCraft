# ProjectCraft roadmap

**Stage: Alpha (M0–M6 vertical slice landed).** Product scope and research
live in [`vision.md`](vision.md); known compatibility gaps are tracked in
[`docs/gaps.md`](docs/gaps.md).

## Milestones

| Id | Scope | Status |
| --- | --- | --- |
| M0 | Core model (`pmcore`): WBS hierarchy, work items (task/milestone/epic/story/bug), custom fields, statuses, priorities, agile sprints & boards, resources, rate tables, calendars, baselines | done |
| M1 | Scheduling engine (`pmsched`): CPM forward/backward pass, FS/SS/FF/SF dependencies with leads/lags, task constraints (`ASAP`/`ALAP`/`SNET`/`SNLT`/`FNET`/`FNLT`/`MSO`/`MFO`), critical path, free/total slack, Monte Carlo three-point PERT risk simulation | done |
| M2 | Agile workflows (`pmcore` + `pmsched` + `ui-egui`): backlogs, sprints, story points, WIP-limited Kanban board, velocity & sprint burndown | done |
| M3 | Resources & Earned Value (`pmsched`): time-phased resource allocation, overallocation detection, priority-rule resource levelling with explanations, baseline variance, full EVM (`PV`, `EV`, `AC`, `BAC`, `SV`, `CV`, `SPI`, `CPI`, `EAC`, `ETC`, `VAC`, `TCPI`, cost curves) | done |
| M4 | Interop & reporting (`pmformat`): `.projectcraft` JSON, CSV, Jira JSON, Microsoft Project MSPDI `.xml`, Primavera P6 `.xer` import/export, headless Gantt SVG and HTML report generation | done |
| M5 | Automation & Studio (`engine`, `mcp`, `ui-egui`, `projectcraft-cli`, `projectcraft`): 38 engine commands, MCP stdio server, egui Studio (Table, Gantt, Board, Workload, EVM/Risk views + TCP control server) | done |
| M6 | Collaboration, Web & Packaging (`pmsync`, `projectcraft-sync`, `projectcraft-web`, `packaging/windows/`): role-enforced change DAG & comment sync, post-merge semantic conflict detector, `wasm32` web app (`cargo xtask wasm`), and Windows WiX v5 MSI + portable zip installer | done |
