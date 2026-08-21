# Dependency hygiene

Audience: contributors and maintainers.

Run the same dependency audit as CI from the workspace root:

```bash
cargo machete --with-metadata
```

The check is required on pull requests. A finding may be removed only after
checking normal source, tests, examples, doctests, build scripts, generated
code, feature definitions, and a minimal-feature build. If a dependency is an
intentional false positive, add the narrowest possible
`package.metadata.cargo-machete.ignored` entry to its owning manifest and
explain the reason next to it.

## 2026-08-21 audit (#448)

The audit classified every finding emitted by cargo-machete 0.9.2:

| Package | Dependency | Classification |
| --- | --- | --- |
| `tower-resilience-fallback` | `tower-layer`, `tower-service` | Removed: the crate imports both traits through `tower`. |
| `tower-resilience-reconnect` | `thiserror`, `tokio-test` | Removed: neither appears in source or tests. |
| `tower-resilience-reconnect` | `tower-resilience-core` | Removed: source does not use the crate; its feature forwarding did not enable the corresponding `tower-resilience-retry` features and was therefore ineffective. |
| `tower-resilience-core` | `serial_test`, `tracing-subscriber` | Removed: neither appears in source or tests. |
| `tower-resilience-bulkhead` | `tower-layer`, `tower-service` | Removed: the crate imports both traits through `tower`. |
| `tower-resilience-outlier` | `tower-layer`, `tower-service` | Removed: the crate imports both traits through `tower`. |
| `tower-resilience-circuitbreaker` | `metrics-util` | Removed: metrics instrumentation uses `metrics`; no recorder from `metrics-util` is created by the library. |
| `tower-resilience-adaptive` | `pin-project-lite` | Removed: no pin-projection macro or type is used. |
| `tower-resilience-hedge` | `pin-project-lite`, `tower-service` | Removed: no pin-projection macro is used, and `Service` is imported through `tower`. |
| `tower-resilience-healthcheck` | `tower-layer` | Removed: the integration test imports `Layer` through its `tower` dev-dependency. |
| `tonic-resilient-greeter` | `rand` | Removed: neither binary nor its build script uses randomness. |
| `tonic-resilient-greeter` | `prost`, `tonic-prost` | Kept and ignored: generated protobuf/message and codec code refers to these crates. |
| `tonic-resilient-greeter` | `tonic-prost-build` | Kept and ignored: `build.rs` calls it directly, but cargo-machete does not scan the build script. |

This left no unexplained findings. Because the remaining exceptions are local,
documented, and exercised by the workspace build, the audit was promoted
directly to a required CI gate rather than introduced as an advisory check.
