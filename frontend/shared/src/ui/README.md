# Shared UI

The views and components both front ends render. Specification §2.7 describes
`frontend/shared` as "components + API client used by both UIs"; this directory
is the components half.

**Written once, rendered twice.** `frontend/web` and `frontend/desktop` import
these files directly — there are no copies. A change here changes both.

## The two aliases

Sharing a view between two applications means the view must not know which
transport it is running over. That inversion is expressed as two build aliases,
configured identically in each application's `vite.config.ts` and
`tsconfig.app.json`:

| Alias | Resolves to | Direction |
|---|---|---|
| `@ui/*` | `frontend/shared/src/ui/*` | the application imports these views |
| `@host/api` | the application's own `src/api.ts` | these views reach the application's transport |

So a view says `import { api } from '@host/api'` and gets `HttpApiClient` in the
web build and `TauriApiClient` in the desktop build, without naming either. Both
export `api: ApiClient`, which is the contract.

Because `@host/api` resolves to a real module rather than an ambient
declaration, these views are typechecked against each application's actual
client — twice, once per build. A view that used something only the HTTP client
offers would fail the desktop typecheck.

## Views, components, and where each belongs

`views/` is the **rendered-twice** contract: a view here is compiled into both
applications, so it may only use `ApiClient` methods both transports genuinely
implement.

`components/` is a **library**. A component here is transport-free — props in,
events out, no `@host/api` — so it can be used by a shared view, by one
application's own view, or by a test harness with no application at all.
`ShotGrid` and `BulkActions` are used by the desktop alone and still live here,
because being transport-free is what makes them measurable: `check:ingest`
mounts them on their own and puts four hundred shots through them.
`TrackLibrary` is here for the same reason: the conflict table is the one part
of geotagging a person has to *judge*, and it can be put in front of a harness
with no application around it.

Some screens genuinely belong to one application, and those live in that
application's `src/views/`:

| View | Where | Why |
|---|---|---|
| `Ingest` | `frontend/desktop/src/views/` | §2.3 puts the card reader on the Mac. The server has no card to read. |
| `Publish` | `frontend/web/src/views/` | The Google refresh token lives on exactly one machine (§2.3). |

The alternative — putting both in `views/` and having each transport throw for
the half it cannot do — was rejected. A capability that exists in the type
system and fails at runtime is worse than one the type system never offered:
the first is discovered by a person pressing a button, the second by a compiler.

## What stays in the applications

`App.vue`, `main.ts` and `api.ts` are per-application and deliberately not here:

- **`App.vue`** — the shells genuinely differ. The web shell carries Firebase
  sign-in; the desktop shell carries the server-reachability indicator and the
  offline banner.
- **`main.ts`** — the desktop uses hash history and has no dashboard route.
- **`api.ts`** — the transport. This is the one file the two builds do not share,
  and the reason everything else can be shared.

## The transport boundary

`check:transport` in each application scans this directory as well as the
application's own `src/`, so a view that reached for `fetch` here would fail
both builds. The allow-list is `src/api.ts` (and `src/auth.ts` on the web),
never anything under `@ui`.

## Type resolution

These files are excluded from the shared package's `tsc` build — `dist/` holds
only `types.ts` and `client.ts`, which are consumed as a compiled library. The
`.vue` files are consumed as source and compiled by each application's Vite
build, which is what lets them see that application's `@host/api`.
