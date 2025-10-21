# Repository Guidelines

## Project Structure & Module Organization
- `Cargo.toml` defines the Cargo workspace; shared infra lives in `crates/tower-resilience-core`.
- Pattern crates live under `crates/tower-resilience-*` and each exports its own builder APIs plus examples.
- Top-level `examples/` mirror README snippets; `tests/` hosts integration and stress suites; `benches/` measures middleware overhead.
- Generated artifacts land in `target/`; keep it out of commits.

## Build, Test, and Development Commands
- `cargo build --workspace` compiles every crate with shared features.
- `cargo test --workspace --all-features` runs unit and integration tests; add `-- --ignored` to exercise stress scenarios.
- `cargo bench --bench happy_path_overhead` benchmarks middleware latency.
- `cargo run --example <name>` runs quick-start samples; pass `-p tower-resilience-<pattern>` for crate-specific demos.
- `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --all` gate style before pushing.

## Coding Style & Naming Conventions
- Standard Rust 4-space indentation with rustfmt; never hand-format generated files.
- All public APIs need doc comments that explain resilience behavior and configuration.
- Crate names follow `tower-resilience-*`; modules use `snake_case`, types use `UpperCamelCase`, traits use `CamelCase`.
- New crates should target Rust 2021 edition; workspace MSRV aligns with Rust 1.64.0 even though the root uses 2024.

## Testing Guidelines
- Write focused unit tests alongside implementations and put cross-crate flows in `tests/`.
- Stress rigs in `tests/stress.rs` stay opt-in; run them with `cargo test --test stress -- --ignored --nocapture` when validating concurrency changes.
- Prefer deterministic seeds for chaos and retry layers to keep CI stable.
- Update examples when APIs change; `cargo run --example circuitbreaker` is part of manual smoke-testing.

## Commit & Pull Request Guidelines
- Use Conventional Commits (`feat:`, `fix:`, `docs:`) with imperative, 72-character subjects.
- Each PR should link issues when relevant, describe behavioral impact, and note testing done.
- Ensure fmt, clippy, and full test suite pass before requesting review; attach benchmark deltas if performance is affected.
- Include screenshots or logs only when they clarify resilience metrics or chaos output.
