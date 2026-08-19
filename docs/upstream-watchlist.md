# Upstream Tower crossover watchlist

This is the durable list of upstream `tower-rs/tower` (and `tower-http`)
issues that can erase a differentiator this project claims, supply a parity
test we should mirror, or hand us an implementation primitive worth adopting.
Each row records local ownership so a release reviewer can tell at a glance
which local claims are backed by resolved work, which are still in progress,
and which upstream items have no local coverage yet.

`Last reviewed` is the date someone last confirmed the row against current
upstream and local state, not the date the row was created.

## Watchlist

| Upstream | What it threatens or offers | Local owner | Status | Last reviewed |
| --- | --- | --- | --- | --- |
| [tower#855] | Upstream circuit breaker proposal; could commoditize our headline circuit-breaker differentiator | [circuitbreaker-tower-comparison.md], [#382], [#384] | Resolved | 2026-08-18 |
| [tower#880], [tower#842], [tower#794] | Rate-limiter accessor API, sliding-window algorithm, and clone-shared state -- accessors and shared-clone admission overlap our rate limiter | [#277] (PR [#279]) for accessors; [#363] (PR [#394]) for shared-clone admission; [#430] (parity tests in `crates/tower-resilience-ratelimiter/tests/upstream_sliding_window_parity.rs`) for the sliding-window algorithm | Resolved | 2026-08-18 |
| [tower#857], [tower#863] | Retry TPS budget semantics; upstream `TpsBudget` design could set the parity bar for our retry budget | [#370] | Resolved | 2026-08-18 |
| [tower#862], [tower#864] | Hedge validation: idempotency/eligibility gating and losing-attempt cancellation | [#369] (PR [#393]) | Resolved | 2026-08-18 |
| [tower#793] | Reject-now (fail-fast) concurrency limiting, as an alternative to Tower's queue-on-full `ConcurrencyLimit` | [#168] (doc), implemented by PR [#170] (`reject_when_full()`) | Resolved | 2026-08-18 |
| [tower#798] | Per-request timeout extraction (timeout derived from the request, not a static duration) | [#167] | Resolved | 2026-08-18 |
| [tower#807] | Arbitrary executor delegation for blocking/parallel work | [#371] (PR [#415]) | Resolved | 2026-08-18 |
| [tower#60] | Reconnect backoff via a connection factory | [#361] (PR [#391]), [reconnect-factory-migration.md] | Resolved | 2026-08-18 |
| [tower#682] | Retry ergonomics (non-`Clone` errors, readiness-before-retry) | [#346] (PR [#350]), [#362] (PR [#392]) | Resolved | 2026-08-18 |
| [tower#696], [tower#859], [tower#866] | Weighted P2C routing and empty-`Steer` validation semantics | [#366] (closed), PR [#401] (merged 2026-08-18) | Resolved | 2026-08-19 |
| [tower-http#688] | Body-frame deadlines for streamed HTTP request/response bodies | [#373] (PR [#403]) | Resolved | 2026-08-18 |

## Maintenance rules

- **Adding an item.** When an upstream Tower or tower-http issue threatens a
  claim, offers a parity test, or hands us a reusable primitive, add a row
  here with the upstream link, what's at stake, and the local owner (an
  issue, a PR, or `no local coverage` if none exists yet).
- **Reviewing an item.** At least once per release, walk every row whose
  status is not `Resolved`, confirm whether upstream state or local coverage
  has changed, and update `Last reviewed`.
- **Closing or replacing an item.** Mark a row `Resolved` once local coverage
  ships and matches the upstream item's scope. If the upstream item is
  closed, superseded, or judged inapplicable, replace the row's content
  with a one-line note explaining why and keep the row for history, or
  remove it if it no longer has any bearing on this project.
- **Parity tests.** When an upstream fix or discussion exposes a shared edge
  case (a bug both projects could hit, or a contract Tower documents that we
  should test too), add a regression or differential test locally and link
  it from the row.
- **Comparison docs.** When upstream behavior actually ships (not just a
  proposal), update the relevant comparison doc (for example
  [circuitbreaker-tower-comparison.md]) so the comparison reflects current
  upstream behavior, not a stale proposal.

[tower#855]: https://github.com/tower-rs/tower/issues/855
[tower#880]: https://github.com/tower-rs/tower/issues/880
[tower#842]: https://github.com/tower-rs/tower/issues/842
[tower#794]: https://github.com/tower-rs/tower/issues/794
[tower#857]: https://github.com/tower-rs/tower/issues/857
[tower#863]: https://github.com/tower-rs/tower/issues/863
[tower#862]: https://github.com/tower-rs/tower/issues/862
[tower#864]: https://github.com/tower-rs/tower/issues/864
[tower#793]: https://github.com/tower-rs/tower/issues/793
[tower#798]: https://github.com/tower-rs/tower/issues/798
[tower#807]: https://github.com/tower-rs/tower/issues/807
[tower#60]: https://github.com/tower-rs/tower/issues/60
[tower#682]: https://github.com/tower-rs/tower/issues/682
[tower#696]: https://github.com/tower-rs/tower/issues/696
[tower#859]: https://github.com/tower-rs/tower/issues/859
[tower#866]: https://github.com/tower-rs/tower/issues/866
[tower-http#688]: https://github.com/tower-rs/tower-http/issues/688

[#167]: https://github.com/joshrotenberg/tower-resilience/issues/167
[#168]: https://github.com/joshrotenberg/tower-resilience/issues/168
[#170]: https://github.com/joshrotenberg/tower-resilience/pull/170
[#277]: https://github.com/joshrotenberg/tower-resilience/issues/277
[#279]: https://github.com/joshrotenberg/tower-resilience/pull/279
[#346]: https://github.com/joshrotenberg/tower-resilience/issues/346
[#350]: https://github.com/joshrotenberg/tower-resilience/pull/350
[#361]: https://github.com/joshrotenberg/tower-resilience/issues/361
[#362]: https://github.com/joshrotenberg/tower-resilience/issues/362
[#363]: https://github.com/joshrotenberg/tower-resilience/issues/363
[#366]: https://github.com/joshrotenberg/tower-resilience/issues/366
[#369]: https://github.com/joshrotenberg/tower-resilience/issues/369
[#370]: https://github.com/joshrotenberg/tower-resilience/issues/370
[#371]: https://github.com/joshrotenberg/tower-resilience/issues/371
[#373]: https://github.com/joshrotenberg/tower-resilience/issues/373
[#382]: https://github.com/joshrotenberg/tower-resilience/issues/382
[#384]: https://github.com/joshrotenberg/tower-resilience/issues/384
[#391]: https://github.com/joshrotenberg/tower-resilience/pull/391
[#392]: https://github.com/joshrotenberg/tower-resilience/pull/392
[#393]: https://github.com/joshrotenberg/tower-resilience/pull/393
[#394]: https://github.com/joshrotenberg/tower-resilience/pull/394
[#401]: https://github.com/joshrotenberg/tower-resilience/pull/401
[#403]: https://github.com/joshrotenberg/tower-resilience/pull/403
[#415]: https://github.com/joshrotenberg/tower-resilience/pull/415
[#430]: https://github.com/joshrotenberg/tower-resilience/issues/430

[circuitbreaker-tower-comparison.md]: circuitbreaker-tower-comparison.md
[reconnect-factory-migration.md]: ../crates/tower-resilience-reconnect/MIGRATION.md
