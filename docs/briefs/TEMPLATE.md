# `<ID>` · <title>

> **Status:** briefed · in progress · reported · changes requested · **approved**
> **Plan:** [`docs/<plan>.md`](../<plan>.md), step `<ID>` · **Branch:** `<id>`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11) and **Gates**. They apply to every line of this work.
- <the plan section this step comes from, and any file whose conventions must be followed>

### Goal

<What changes, and why — one paragraph. The why matters: it is what lets the implementer make the
small decisions the brief did not foresee.>

### Change

<Files to create or modify, by path, and what happens in each. Specific enough that two
implementers would produce the same structure.>

### Do not touch

<Files and areas outside this step. Anything not listed in *Change* is out of scope by default;
name the tempting ones here.>

### Acceptance

Each one checkable by someone who did not do the work. Numbers wherever there are numbers.

- [ ] <criterion>
- [ ] Test counts: `<before>` → `<after>`, every test accounted for.

### Gates

```sh
<the exact commands, each with its expected result>
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; the **actual
output** of every gate — counts and failures, never just "passed"; and anything you could not
verify, said plainly. Do not tick the acceptance boxes above; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** — or **Changes requested:**

1. <specific, actionable, one change per item>
