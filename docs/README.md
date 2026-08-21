# docs/

## What belongs here

Crate users read [docs.rs](https://docs.rs) and the README, not this
directory. User-facing content -- usage examples, migration guides, anything
a downstream consumer needs -- belongs in rustdoc (`//!` crate docs, or a
`MIGRATION.md` in the crate directory included via `#[doc =
include_str!(...)]`) so it ships with the published crate.

`docs/` holds contributor and maintainer artifacts only: audits, contract
matrices, upstream-tracking lists, and review checklists that inform how the
project is built and reviewed, not how it is used. Every file listed below
states its audience and the process that keeps it current. A new addition
must do the same, or it does not belong here.

## Index

| File | Audience | Role |
| --- | --- | --- |
| [tower-contract-matrix.md](tower-contract-matrix.md) | Contributors | Source of truth for `tower::Service` contract coverage per crate. Updated whenever a new or changed execution path lands; gaps link to the tracking issue. |
| [tower-service-review-checklist.md](tower-service-review-checklist.md) | Contributors | Short operational checklist to paste into a PR that adds or materially changes a `tower::Service` implementation. Companion to the contract matrix. |
| [tower-api-surface-audit.md](tower-api-surface-audit.md) | Contributors, maintainer | Durable record of the API-surface audit against core Tower's own conventions (boxing, trait bounds, allocations, Tokio coupling, accessors). Reference baseline for future public-API changes. |
| [circuitbreaker-tower-comparison.md](circuitbreaker-tower-comparison.md) | Contributors, maintainer | Pinned comparison against the upstream `tower-rs/tower#855` circuit-breaker proposal, including the RMQTT/GovCraft-Acton integration review. Updated when upstream behavior actually ships, not on every proposal revision. |
| [upstream-watchlist.md](upstream-watchlist.md) | Maintainer | Durable list of upstream `tower`/`tower-http` issues that could erase a local differentiator, supply a parity test, or hand over a reusable primitive. Reviewed at least once per release. |
| [release-process.md](release-process.md) | Maintainer | Reproducible publish preflight, ordered release monitoring, and partial-failure recovery runbook. Reviewed for every release PR. |

## Maintaining this index

Adding a file to `docs/` requires adding a row here with its audience and
the process that keeps it current. Removing or moving a file (for example,
into a crate's rustdoc) requires removing its row and checking the whole
repository for links to the old path.
