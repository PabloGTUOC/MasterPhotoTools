## Phase 7a — Shared views

**Status:** complete. Not a build-plan phase; it closes deviation 5 of Phase 7 while the two copies
were still byte-identical and the move was mechanical.

### Why now

Phase 7 shipped `frontend/desktop` by copying six views and four supporting files out of
`frontend/web`. That was the smaller change at the time, but specification §2.7 describes
`frontend/shared` as "components + API client used by both UIs", and every phase from here adds
views — Phase 13 adds ingest views to the desktop specifically. Done today the copies were identical
and this was a file move; done after Phase 13 it would have been a merge of two drifted trees.

### Delivered

Ten files moved to `frontend/shared/src/ui`:

| | |
|---|---|
| `style.css` | |
| `components/` | `JobProgress.vue`, `ToolPage.vue` |
| `views/` | `ContactSheet`, `Dashboard`, `Dates`, `ImageTool`, `Library`, `Rename`, `Transform` |

**The inversion.** A view shared between two applications must not know which transport it runs
over. That is expressed as two build aliases, configured identically in each application's
`vite.config.ts` and `tsconfig.app.json`:

- `@ui/*` → `frontend/shared/src/ui/*` — how an application imports the views.
- `@host/api` → that application's own `src/api.ts` — how the views reach its transport.

So a view says `import { api } from '@host/api'` and gets `HttpApiClient` in the web build and
`TauriApiClient` in the desktop build without naming either. **The one-line import change in eight
files is the entire diff to the view sources.**

Because `@host/api` resolves to a real module rather than an ambient declaration, **the views are
typechecked twice — once per application, against that application's actual client.** A view that
reached for something only the HTTP client offers would fail the desktop typecheck. That is stronger
than the single check against an abstract interface that a conventional shared package would give.

`frontend/shared/src/ui/README.md` documents the arrangement, including what deliberately stays
per-application: `App.vue` (the shells genuinely differ — Firebase sign-in versus the server
indicator), `main.ts` (hash history, no dashboard route) and `api.ts` (the transport itself).

### The boundary checks now cover the shared views

`check:transport` previously scanned each application's `src/`, which after the move was almost
empty — the check would have passed while enforcing nothing. It now scans the shared views as well,
and gained two assertions:

1. **A missing scan root is a failure**, not a silent pass. A check that stops finding its files must
   say so.
2. **The shared views may import `@host/api` and nothing else.** They compile into both front ends,
   so anything they take from `@host/` has to exist in both. `@host/auth` exists only on the web, and
   reaching for it would couple the shared views to one application.

All three failure modes were exercised rather than assumed:

| Injected fault | Result |
|---|---|
| A shared view calling `fetch()` | `../shared/src/ui/views/Dashboard.vue: uses fetch()` |
| A shared view importing `@host/auth` | `imports @host/auth, which only one front end provides` |
| `frontend/shared/src/ui` moved away | `scan root does not exist` |

### A real bug found on the way

**`frontend/web/vite.config.js` and `vite.config.d.ts` were tracked build output, and Vite prefers
`vite.config.js` over `vite.config.ts`.** The web application had been building against a compiled
snapshot of its configuration taken before `noEmit` was added to `tsconfig.node.json`. It went
unnoticed because the snapshot was still equivalent — it stopped being equivalent the moment this
change added aliases, which is how it surfaced. Both files are deleted and both applications now
ignore that emission path.

### Acceptance

- [x] `npm --prefix frontend/shared run build` — `dist/` unchanged, still the client library alone
- [x] `npm --prefix frontend/web run typecheck` clean
- [x] `npm --prefix frontend/web run build` clean
- [x] `npm --prefix frontend/web run check:transport` — 14 files scanned
- [x] `npm --prefix frontend/web run check:layout` — all 9 routes clean at 390×844
- [x] `npm --prefix frontend/desktop run typecheck` clean
- [x] `npm --prefix frontend/desktop run build` clean
- [x] `npm --prefix frontend/desktop run check:transport` — 13 files scanned
- [x] Each injected violation above fails the check it targets

### Measurements

**Both bundles are byte-for-byte the size they were before the move**, which is the evidence that
this was behaviour-preserving:

| | Before | After |
|---|---|---|
| Web | 280.21 kB / 78.30 kB gzipped, 72 modules | identical |
| Desktop | 117.55 kB / 44.04 kB gzipped, 56 modules | identical |

### Deviations

1. **`vue` added to `frontend/shared` as a dev and peer dependency.** The shared views import it, and
   TypeScript resolves modules from the importing file's location — without it the views could not be
   typechecked. Recorded per G8, though `vue` is not new to the project, only newly declared in this
   package.
2. **`resolve.dedupe: ['vue', 'vue-router']` in both Vite configs.** Each application has its own
   `node_modules`, so the shared views would otherwise resolve `vue` from
   `frontend/shared/node_modules` and a build would bundle two Vue runtimes — under which reactivity
   fails quietly rather than loudly. The dedupe is what makes declaring `vue` in `shared` safe.
3. **The shared views are consumed as source, not as compiled output.** `frontend/shared/tsconfig.json`
   excludes `src/ui`, so `dist/` is still exactly the client library it was; the `.vue` files are
   compiled by each application's Vite build. That is what allows `@host/api` to resolve differently
   per build — a pre-compiled shared package could not do it.
4. **`composite: true` requires every input file to be listed**, so each application's
   `tsconfig.app.json` includes `../shared/src/ui/**` explicitly rather than picking the files up
   through the import graph.

### Notes for the next phase

- **Phase 13's ingest views belong in `frontend/shared/src/ui`**, even the desktop-only ones. Routing
  is per-application, so a view can live in the shared tree and be reachable from one router only;
  putting it there gets it typechecked against both clients at no cost.
- **`ToolPage` is where the preview-before-apply gate lives**, and it is now shared. Phase 13's
  "publishing must be refused without a dry run" requirement should extend that component rather than
  add a second gate beside it.
