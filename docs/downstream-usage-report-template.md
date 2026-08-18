# Downstream usage report template

Copy this into a new issue on your own project, a GitHub Discussion reply,
or an email -- whatever's easiest. Every field is optional; partial answers
are still useful. This exists so the tower-resilience maintainer can build
an accurate picture of real-world usage without guessing from crates.io
metadata alone.

```markdown
## tower-resilience usage report

**Project:** <name and repo link>
**tower-resilience crate(s) used:** <e.g. tower-resilience-circuitbreaker, tower-resilience-bulkhead>
**Version currently pinned:** <e.g. "0.12", "^0.10">
**Dependency shape:** <mandatory / optional+feature-gated / dev-only>

### What you're protecting

<e.g. "outbound gRPC calls to a storage backend", "inbound HTTP routes
behind axum", "a connection pool to an internal service">

### How it's wired up

<a short code snippet or description is ideal -- builder config used,
whether you hold a Handle, whether you use build() or build_with_handle(),
any custom classifier, layer ordering relative to other tower layers>

### Friction encountered (if any)

<anything that required a workaround, felt awkward, was underdocumented,
or is a capability you wanted but had to build yourself. Be blunt -- this
is the most useful section.>

### Anything you'd like changed or added

<feature requests, API shape suggestions, anything>

### Okay to list publicly as a downstream user?

<yes / no / yes but without a code snippet / ask me first before quoting
anything>

### Anything else worth knowing

<upgrade pain, version history, whatever doesn't fit above>
```

## Why this template exists

Reverse-dependency data from crates.io tells you *that* a project depends
on a crate, but not *how* -- whether it's load-bearing or vestigial,
whether the integration is happy or held together with a workaround, or
whether the maintainer would even want to be named. This template turns
"I found your Cargo.toml" into an actual conversation, and gives every
respondent the same shape so answers are easy to compare across projects
over time.

See [`docs/downstream-users.md`](downstream-users.md) for the audit this
template supports and the draft outreach messages that link to it.
