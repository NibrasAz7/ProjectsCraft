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
  CSV, PDF, Jira JSON/XML, `.mpp`/MSPDI XML schedules, Primavera P6 `.xer`
  exchange, iCalendar — so arriving and leaving are both free.

## What "complete" means

1. **Plan** — WBS, milestones, phases; effort and duration estimates.
2. **Schedule** — finish-start/finish-finish/start-start/start-finish
   dependencies with leads and lags, constraints, custom calendars,
   critical path, slack, baselines, baseline-vs-actual variance, and
   background Monte Carlo schedule-risk simulation.
3. **Work** — kanban/scrum boards, sprints, backlogs, custom fields,
   workflow states, subtasks, checklists.
4. **Resource** — people, skills, availability calendars, multiple
   resource assignments, over-allocation warnings, automatic levelling,
   workload heatmaps.
5. **Track** — time logging, rate tables, fixed task costs, cost curves,
   actuals vs baseline, earned value (EV/PV/AC, SV/CV, SPI/CPI, EAC).
6. **Report** — burndowns, Gantt PDF/CSV exports, portfolio dashboards,
   saved filters, printable reports.
7. **Automate** — rules ("when state → done, notify owner"), templates,
   recurring tasks, CLI and MCP automation.

## Milestone sketch

- **M0 — Core model + table/board.** Work items, fields, WBS, saved views.
- **M1 — Scheduling engine.** Dependencies, calendars, parallel CPM (10k+
  tasks), responsive 60 FPS Gantt view, Monte Carlo risk simulation.
- **M2 — Agile.** Sprints, backlogs, boards, burndown.
- **M3 — Resources & EVM.** Rate tables, fixed costs, cost curves,
  levelling, workload, baselines, earned value (SV/CV/SPI/CPI).
- **M4 — Interop.** Jira/CSV/PDF/MPP/MSPDI XML/Primavera `.xer`
  import-export, `.projectcraft` format frozen.
- **M5 — Everywhere.** Web app (WASM), collaboration, packaging per the
  ArtCraft release conventions.

## The name

*ProjectCraft* (or *GanttCraft*, per the Remade `microsoft-project`
request's `suggested_names`) — the project-management member of the
ArtCraft family: one open tool for planning, scheduling, working and
reporting, without the seams.

---

## Research: feature landscape

What the incumbents actually ship, and where the opening is.

- **Jira** — custom issue types/fields/statuses, visual workflow engine,
  scrum + kanban boards (estimation, velocity, burndown, WIP limits,
  swimlanes), roadmaps, releases, JQL, dashboards, automation rules,
  huge marketplace. *Complaints:* bureaucratic complexity, needs a
  dedicated admin, hostile to non-devs, per-seat pricing, forced
  cloud migration (Jira Data Center EOL March 2029 is a real deadline
  driver).
- **monday.com** — board→group→item hierarchy, 30+ column types, many
  views (table, kanban, Gantt with critical path & baseline, workload,
  forms), dashboards, automations. *Complaints:* no real scheduling —
  due dates ignore capacity and dependencies; automation action quotas;
  no offline or self-host.
- **Asana** — clean UX, goals/OKRs, portfolios, workload, workflow
  builder. *Complaints:* no true scheduling engine (dependencies don't
  auto-reschedule, no levelling); time tracking and reporting gated to
  expensive tiers.
- **ClickUp** — deep hierarchy, 15+ views, native docs, native time
  tracking, 50+ dashboard widgets. *Complaints:* slowness at scale
  (the Gantt especially), feature overload, notification noise.
- **Microsoft Project** — the gold-standard scheduling engine: CPM
  forward/backward pass, four dependency types with leads/lags,
  constraints and calendars, baselines + variance, resource levelling,
  earned value (SPI/CPI/EAC). *Complaints:* dated UI, Windows-only,
  expensive, no real collaboration, agile bolted on.

**The cross-cutting grievance:** per-seat SaaS pricing plus cloud
lock-in — and every work-manager ships a fake Gantt while MS Project
ships real scheduling with no collaboration. Nobody covers both.

### The union checklist

Planning (WBS, custom fields, milestones, goals, portfolios, docs,
templates, intake forms) · Scheduling (Gantt, CPM, FS/SS/FF/SF +
leads/lags, constraints, calendars, baselines + variance, critical
path) · Agile (backlog, sprints, points, velocity, burndown, boards)
· Resources (pool with calendars and cost rates, units %, workload
heatmaps, levelling) · Time (timers, actuals vs estimates, timesheets)
· Reporting (cross-project rollups, EVM, saved-filter query language)
· Automation (trigger–condition–action, recurring tasks, webhooks)
· Interop (`.mpp`, Jira CSV/JSON, full API) · Admin (roles, guests,
audit, offline).

### Hardest problems, honestly ranked

1. **Incremental scheduling** — CPM itself is O(V+E), but real engines
   need calendar-aware lags, SF/FF edges, constraints and *incremental*
   reschedule on edit, not batch recompute.
2. **Resource levelling** — RCPSP is NP-hard; MS Project's is a
   proprietary black box. Ship priority-rule heuristics that produce
   *sensible* results with user-visible explanations.
3. **Real-time collaboration** — the deepest architectural commitment
   (see below); it shapes the data layer, so decide it first.

## Research: user journeys

Six personas the product must serve, and what each demands.

- **Project manager / PMO lead.** Plan (WBS → estimates → dependencies
  → assign → baseline), track (actuals vs baseline → variance →
  critical path → re-plan), report (status digest for steering).
  *Today they keep the same plan twice — Jira for the team, `.mpp` or
  Excel for the PMO — and the copies drift within a week.* One document,
  both views, kills the duplication.
- **Team member.** A keyboard-first **"My Work"** inbox aggregating
  assignments across all projects; one-click status transitions;
  minimal required fields. *Jira's forms make updating the ticket
  slower than doing the work* — friction here kills adoption.
- **Scrum master.** Refine backlog (stack-rank, split, estimate, DoD) →
  plan sprint to capacity → run the board (swimlanes, WIP limits) →
  burndown/velocity review → unfinished work rolls back automatically.
  Lightweight automations ("close parent when subtasks done") come free
  from the command engine.
- **Resource manager.** Portfolio-wide workload heatmap → what-if
  sandbox (model reallocation without touching the live plan) →
  forecast demand by role/skill. *Capacity data is siloed everywhere
  else — the heatmap answers "can we take this on?" in one look.*
- **Executive / portfolio stakeholder.** RAG rollup of all initiatives
  → drill into the red ones → exception alerts (schedule variance >10%)
  → scheduled email digest. *Free read-only access — execs will never
  buy a seat or learn the tool.*
- **External guest / client.** Item-scoped sharing: a client sees a
  curated milestone view and can comment/approve; contractors see only
  their tasks; intake forms land in a triage inbox. *Per-seat guest
  pricing is the incumbents' hidden tax — local-first makes free
  guests natural.*

**Cross-cutting implication:** one data model — a task is a WBS node,
a board card and a backlog item simultaneously — and every journey
above is expressible as CLI/MCP commands.

## Research: collaboration model

The incumbents force a binary choice — Jira-class multiplayer *or*
file ownership. Local-first's wedge is offering both.

**Recommendation: Automerge document + dumb relay server.**

- **Store:** `.projectcraft` as an [Automerge](https://automerge.org)
  binary document — Rust-native, WASM-compatible, proven sync protocol.
  Every engine command mutates the doc as one change; undo/redo and the
  activity feed fall out of the change history for free. (Loro is the
  faster alternative worth benchmarking.)
- **Solo mode:** file only, full history, zero infrastructure, works
  fully offline. Git-exportable snapshot for backup.
- **Team mode:** self-hosted `projectcraft-sync` — a single-binary
  WebSocket relay persisting change-sets to SQLite. It's a **mailbox,
  not a database**: no application logic, so it can't corrupt data and
  any peer can restore it. Deploy = one Docker image.
- **Permissions (server-enforced):** Owner / Editor / Commenter /
  Viewer (read-only sync) / Guest (single project). The server checks
  tokens before relaying changes; the UI also hides disallowed
  commands.
- **Social layer:** comments, mentions and reactions are ordinary CRDT
  collections inside the doc — they sync and merge like tasks.
  Notifications = clients watching the change stream; presence
  (cursors/avatars) ships last, over ephemeral messages.
- **Offline:** every peer holds the whole doc. Disconnect → keep
  editing; reconnect → sync converges automatically.

**Known risks:** semantic conflicts survive CRDTs (two people
rescheduling the same task merges cleanly but may produce an impossible
plan — flag violations in the activity feed post-merge instead of
rejecting); history growth needs compaction + shallow snapshots for
new joiners; the change DAG doubles as the enterprise audit log.
**Avoid:** a custom CRDT, business logic in the server, and binary
file-sharing as anything but an export path.
