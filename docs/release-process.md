# Release preflight and recovery

Audience: maintainers preparing, monitoring, or recovering a workspace release.

Review this runbook for every release PR. Update it whenever the publishable
workspace shape, release-plz workflow, or crates.io recovery procedure changes.

## Non-publishing preflight

From the repository root, run:

```bash
python3 scripts/publish_preflight.py
```

The command reads Cargo metadata, excludes every package with
`publish = false`, calculates a topological order for the remaining workspace
packages, validates their metadata and internal dependency versions, checks
each `cargo package --list`, and produces all `.crate` archives in one
non-publishing `cargo package --no-verify` invocation. It then verifies that:

- README, license metadata, changelog, targets, and required package files are
  present;
- path dependencies carry the synchronized internal version and git
  dependencies are absent;
- each archive exactly matches Cargo's file list and contains a normalized
  manifest without path or git sources; and
- packaging did not modify a source manifest or the workspace lockfile.

The preflight uses `--allow-dirty` so it can be run while preparing a PR, but
it snapshots manifests and lockfiles and fails if Cargo changes them. It does
not publish, upload, tag, or create a GitHub release.

CI runs this same command on every pull request, which necessarily includes
release-plz release PRs. Its deliberately malformed path-only dependency
fixture is covered by `python3 -m unittest scripts.tests.test_publish_preflight`.

## Why this is not `release-plz release --dry-run`

The workspace crates share a synchronized version. Before a release starts,
that version of `tower-resilience-core` does not exist on crates.io. A
release-plz dry run does not actually publish core, so later packages can fail
resolution even though the real ordered release would succeed.

The preflight instead selects every publishable package in one Cargo packaging
operation. Cargo can then prepare interdependent packages under the assumption
that the selected versions will be published together. `--no-verify` avoids
trying to rebuild an extracted dependent archive against a version that is not
yet in the registry; normal workspace CI still builds and tests all source,
and the preflight inspects the exact archives and normalized manifests.

Do not use `cargo package --workspace` here. It selects the repository test
harness and integration examples, which intentionally have `publish = false`
and path-only dependencies.

## Publication order

Treat the order printed by the preflight as authoritative. The dependency
stages are:

1. `tower-resilience-core`.
2. Pattern crates whose internal dependencies are now available. Independent
   pattern crates may publish in the same stage; `tower-resilience-reconnect`
   waits for both core and retry.
3. `tower-resilience`, after every optional pattern crate is available.

Release-plz performs the actual publish. The preflight calculates the order
from Cargo metadata instead of maintaining a second hard-coded crate list.

## Monitored publish checklist

1. Confirm the release PR's publish preflight and normal CI are green, it has
   no conflicts, and its generated changelog matches the intended release.
2. Merge the release PR and immediately locate the `Release` workflow run for
   that merge commit with `gh run list --workflow Release`.
3. Watch it through completion with `gh run watch <run-id> --exit-status`.
4. Check the workflow log for the order and outcome of every crate. Do not
   treat a Git tag or GitHub release alone as proof that all crates reached
   crates.io.
5. Verify the expected version of core, each pattern crate, and the facade on
   crates.io. Allow for registry-index propagation before diagnosing a missing
   dependency as a permanent failure.
6. Confirm the expected tags and GitHub releases only after the registry set is
   complete.

## Partial-failure recovery

1. Stop and record the failed workflow run, failing crate, and last confirmed
   published crate. Do not delete tags, yank successful publications, or try
   to overwrite a published version.
2. Inventory every expected crate/version on crates.io and split the list into
   already published and still missing. Recheck after index propagation.
3. For a transient registry or index error, rerun the failed Release workflow
   job. [`release-plz release` publishes unpublished
   packages](https://release-plz.dev/docs/github/quickstart); confirm in the
   rerun log that already-published versions are skipped and only the missing
   set continues. Monitor the rerun as closely as the original.
4. For a source, manifest, or archive defect, fix it on `main`. Published
   archives are immutable: prepare a new patch version for any changed crate
   and its dependents rather than attempting to reuse an already-published
   version.
5. Run the preflight on the recovery branch, require green CI, and document the
   exact published/missing boundary in the recovery PR.
6. After recovery, verify the complete crates.io set, tags, GitHub releases,
   and the next release-plz PR. Record any new failure mode in this runbook.
