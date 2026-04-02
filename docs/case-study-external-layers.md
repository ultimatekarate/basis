# Case Study: External Layers

## The Change

Basis's Placement axis had three exception mechanisms: `depends_on`, `boundaries.rules`, and `boundaries.external`. Three exceptions to one axis suggested structural incoherence — either a missing fifth axis or a unification waiting to happen.

After extensive design exploration (including analogies to the parallel postulate in Euclidean geometry, measure-theoretic framings, coupling constraints, and a proposed "Intent" axis), the resolution was to extend Placement itself: treat external packages as first-class layers marked `external: true`. Internal layers access external packages through `depends_on` — the same structural dependency mechanism that already governs internal code.

Three exception mechanisms became two. `boundaries.external` was eliminated entirely. External dependency control was unified into the layer DAG.

### Before

```yaml
layers:
  dictionary:
    packages: ["basis-cli/src/spec"]
    rules: { purity: strict }
    depends_on: []

boundaries:
  enabled: true
  external:
    dictionary:
      allow: ["serde", "thiserror"]
      deny_patterns: ["tokio", "clap"]
```

### After

```yaml
layers:
  serialization:
    external: true
    packages: [serde, thiserror]

  dictionary:
    packages: ["basis-cli/src/spec"]
    rules: { purity: strict }
    depends_on: [serialization]

boundaries:
  enabled: true
```

The policy is the same. The expression is structural.

## Scope

| Category | Count |
|---|---|
| Files modified | 9 |
| Functions added | 4 (`build_external_map`, `external_entry_matches`, `resolve_external_layer`, `has_wildcard_external_access`, `is_lang_internal_import`) |
| Unit tests added | ~30 |
| Total tests after | 435 (409 unit + 26 integration), all passing |
| Lines of new logic | ~80 |
| Lines of new tests | ~200 |

The modified files, by layer:

| Layer | Files |
|---|---|
| dictionary | `spec.rs` (added `external: bool` to `Layer`) |
| laboratory | `validate.rs` (external layer validation), `placement.rs` (external layer checking), `report.rs` (test fix) |
| basis-runner | `check/purity.rs` (test fix), `infer/mod.rs` (test fix), `tests/check_tests.rs` (test fix) |
| spec | `basis.yaml` (migrated to external layers) |
| docs | `guide.md` (documented external layers) |

## What Basis Made Easy

### Placement was never ambiguous

At no point during implementation was there a question about where code belonged. The `basis.yaml` spec made it immediately obvious:

- **`external: bool` field on `Layer`** — this is a data type. It goes in dictionary (`spec.rs`). There is no other option.
- **`build_external_map`, `external_entry_matches`, `resolve_external_layer`** — these are pure logic operating on spec data. They go in laboratory (`placement.rs`). There is no other option.
- **`validate_external_layers`** — pure validation logic. It goes in laboratory (`validate.rs`). There is no other option.
- **The `basis.yaml` migration** — configuration. It's the spec itself.

Five functions, zero placement decisions. The architecture collapsed the design space so completely that "where does this go?" was never a question that needed answering.

This is the same effect described in the `extends:` case study in the guide. The spec didn't need to catch violations because it made the architecture so legible that violations didn't occur.

### Blast radius was legible

Adding `external: bool` to the `Layer` struct in dictionary broke every `Layer {}` construction across six files. This is a Rust exhaustiveness property, not a Basis one — but Basis made the blast radius *legible*. Every file that constructs a `Layer` is in a layer that `depends_on: [dictionary]`. You could enumerate the affected files by reading the DAG.

The fix was mechanical: add `external: false` to every existing construction. Tedious, but never uncertain.

### The dependency DAG caught a real spec error

After migrating `basis.yaml` to external layers, the first end-to-end run reported:

```
error[B001]: import 'clap::' violates boundary (basis-runner -> cli)
  = note: layer 'basis-runner' does not depend on external layer 'cli'
```

`basis-runner` imports `clap` but didn't list `cli` in its `depends_on`. Under the old `boundaries.external` system with `allow: ["*"]`, this was invisible — everything was allowed. Under external layers, the dependency must be declared. The fix was one line: add `cli` to `basis-runner`'s `depends_on`.

This is a genuine improvement in architectural visibility. The old system silently allowed all external imports for `basis-runner`. The new system requires explicit declaration, and the missing declaration was caught immediately.

## What Basis Caught

The end-to-end verification (`basis check --spec basis.yaml .`) caught two problems:

1. **Missing `cli` dependency** — `basis-runner` imported `clap` without depending on the `cli` external layer. The violation was real: the spec didn't express the dependency that existed in code.

2. **Language-internal imports misclassified** — Rust `crate::`, `super::`, `self::`, and `std::` imports were being checked against external layers. `crate::spec::BasisSpec` normalized to `crate/spec/BasisSpec`, which didn't match any internal layer path, fell through to external layer checking, and matched the wildcard `*` in `all-external`. The fix was a new guard function (`is_lang_internal_import`) that skips language-internal imports from external layer checking.

Both problems were caught by Basis enforcing its own spec. Neither would have been caught by unit tests alone — they required the full integration of spec + checker + real codebase.

## What Basis Couldn't Catch

The `crate::/std::` bug deserves scrutiny. The placement checker was misclassifying language-internal imports as external packages. This is a logic error *in the checker itself*. Basis cannot detect that its own checker has a logic error — the broken ruler cannot measure itself.

The bug was caught by running `basis check` against a real codebase, not by any governance mechanism. The end-to-end verification step in the implementation plan specified exactly this:

> `basis-cli check --spec basis.yaml .` — same results as before migration

This is a testing practice, not a governance one. Basis made the *architecture* correct (every function was in the right layer, no IO leaked into laboratory, no purity violations). But architectural correctness does not imply logical correctness. A pure function in the right layer can still have a bug.

This is a fundamental limit of structural governance: it governs *where* code lives and *what* it's allowed to do, not *whether* it does those things correctly.

## The Self-Governance Test

This change modified Placement while being governed by Placement. That's the hardest test of self-governance — changing the enforcement mechanism under its own enforcement.

What this proves:

- **The architecture is stable under self-modification.** Adding external layer support required changes in dictionary, laboratory, and the spec. At no point did the change destabilize the layer structure or require moving code between layers. The architecture absorbed a significant new feature without structural rearrangement.

- **The dependency DAG is load-bearing.** The missing `cli` dependency was caught because the DAG is enforced, not advisory. Under a weaker system (documentation, convention, code review), this dependency would have been silently implicit.

- **Passive clarity scales.** An AI agent (Claude) implemented the entire feature — design exploration, implementation, testing, migration, documentation — guided primarily by reading `basis.yaml`. The spec's legibility meant the agent never placed code in the wrong layer, never introduced a purity violation, and never needed to be corrected on architectural decisions. The corrections that occurred were logical (the `crate::` bug), not structural.

- **Self-governance has a fixed-point problem.** The checker cannot check itself. This is not a flaw in Basis — it's a fundamental property of any self-referential system. The mitigation is the same as in any compiler: end-to-end testing against known inputs. Basis governs structure; testing governs correctness.

## Metrics

| Metric | Value |
|---|---|
| Design exploration | ~3 hours (parallel postulate, coupling, Intent axis, external layers) |
| Implementation | All code correct on first write (zero placement violations during development) |
| Bugs caught by `basis check` | 2 (missing dependency, import misclassification) |
| Bugs caught by unit tests | 0 (all logic was correct; bugs were integration-level) |
| Architecture changes required | 0 (no files moved between layers, no dependency arrows changed) |
| Violations during development | 0 (passive clarity prevented structural errors) |
| Final test count | 435, all passing |
