# ProjectCraft — Vision

A complete, open-source project-management suite — the clean-room,
Rust-native alternative to **Jira, monday.com, Asana, ClickUp and
Microsoft Project** in a single tool.

> Built the **ArtCraft** way: the engineering conventions, layering and
> product standards of the [storytold](https://github.com/storytold)
> Craft app suite. ProjectCraft follows that vision and contributes back
> to it; the suite itself is developed by storytold.

## The problem

Project management software is fragmented along the exact lines teams
don't work along:

- **Issue trackers** (Jira) do agile boards and workflows but are heavy,
  cloud-locked and hostile to scheduling.
- **Work managers** (monday.com, Asana, ClickUp) do friendly views and
  automations but crumble on real dependencies, baselines and capacity.
- **Schedulers** (Microsoft Project) do CPM, levelling and baselines but
  are alien to everyone who isn't a certified planner.

Teams stitch all three together with exports, plugins and sync jobs —
and pay per seat for the privilege of having their own data trapped in
someone else's cloud.

## The product

**One engine, every lens.** ProjectCraft holds a single project model —
work items, dependencies, resources, calendars, estimates, baselines —
and renders it through every view the incumbents split into separate
products:

| Capability | Replaces |
| --- | --- |
| Issues, epics, custom fields, workflows, sprint boards, backlogs | Jira |
| Table, board, calendar, dashboard views; automations; docs-lite | monday.com / Asana / ClickUp |
| Gantt, critical path, predecessors/constraints, baselines, resource levelling | Microsoft Project |
| Portfolio roll-ups across projects, workload heatmaps | all of them, badly |

A task on a kanban card, a bar on a Gantt and a line in a cost report are
the same object — change it anywhere and every view agrees, because
there is one document, not five integrations.

## The ArtCraft way

- **Rust only.** Native egui/eframe desktop app; the same Rust compiled
  to WebAssembly for the web. No Electron, no webview shell.
- **Clean-room.** Behaviour is designed from public specifications,
  published scheduling algorithms (CPM, PERT, resource levelling) and
  observed UX — never copied code, assets or proprietary formats.
- **Never crash.** No panics, no `unsafe`; untrusted project files and
  imports are hostile input — every failure is a `Result` the user can
  act on.
- **Everything is a command.** Every action — create a task, set a
  dependency, rebaseline, level resources — is a command in the engine,
  reachable identically from the UI, `projectcraft-cli`, the control
  channel and MCP. Agents are first-class users.
- **Local-first.** A project is a folder; the native `.projectcraft`
  file is yours. Offline by default; sync is a feature, not a ransom.
- **No format traps.** Import and export the real interchange formats —
  CSV, Jira JSON/XML, `.mpp`/`MPXJ`-class schedules, iCalendar — so
  arriving and leaving are both free.

## What "complete" means

1. **Plan** — WBS, milestones, phases; effort and duration estimates.
2. **Schedule** — finish-start/finish-finish/start-start dependencies,
   constraints, calendars, critical path, slack, baselines and variance.
3. **Work** — kanban/scrum boards, sprints, backlogs, custom fields,
   workflow states, subtasks, checklists.
4. **Resource** — people, skills, availability calendars, allocation,
   levelling, workload heatmaps.
5. **Track** — time logging, actuals vs baseline, earned value
   (EV/PV/AC, SPI/CPI).
6. **Report** — burndowns, Gantt exports, portfolio dashboards,
   saved filters, printable reports.
7. **Automate** — rules ("when state → done, notify owner"), templates,
   recurring tasks.

## Milestone sketch

- **M0 — Core model + table/board.** Work items, fields, WBS, saved views.
- **M1 — Scheduling engine.** Dependencies, calendars, CPM, Gantt view.
- **M2 — Agile.** Sprints, backlogs, boards, burndown.
- **M3 — Resources & EVM.** Levelling, workload, baselines, earned value.
- **M4 — Interop.** Jira/CSV/MPP import-export, `.projectcraft` format frozen.
- **M5 — Everywhere.** Web app (WASM), collaboration, packaging per the
  ArtCraft release conventions.

## The name

*ProjectCraft* — the project-management member of the ArtCraft family:
one open tool for planning, scheduling, working and reporting, without
the seams.
