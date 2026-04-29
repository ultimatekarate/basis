[![CI](https://github.com/ultimatekarate/basis/actions/workflows/ci.yml/badge.svg)](https://github.com/ultimatekarate/basis/actions/workflows/ci.yml)

# Basis

LLMs amplify whatever the developer already is. Good architecture plus LLM produces correct code at machine speed. Bad architecture plus LLM produces incorrect code at machine speed.

Basis makes the architecture enforceable. You define your architectural rules in a single `basis.yaml`. Basis rejects any code — human or AI-generated — that violates them. Wrong imports, raw primitives where branded types are required, missing enum arms, IO in a pure layer — all caught before the code is committed.

```
error[B002]: parameter 'user_id' uses raw 'str' instead of branded newtype
  --> src/models/types.py:21
  = help: use UserId instead of str

error[B003]: match on 'OrderStatus' is not exhaustive
  --> src/logic/handler.py:14
  = note: missing variants: Cancelled, Shipped
  = help: add case arms for Cancelled, Shipped or add a wildcard (_) pattern

error: aborting due to 2 basis violation(s)
```

## Four Axes

Every architectural governance rule we've encountered (so far) reduces to a constraint along one of four axes:

| Axis | Governs | Error |
| ------ | --------- | ------- |
| **Values** | Data representation — `UserId` cannot be confused with `OrderId` | B002 |
| **Placement** | Code location — a data module cannot import IO | B001 |
| **Completeness** | Case handling — every union variant must be addressed | B003 |
| **Purity** | Side effects — a pure layer cannot perform IO | B004 |

Violations are compiler errors, not warnings. `basis check` returns a non-zero exit code. CI blocks the merge based on your rules. The architecture cannot decay.

## Languages

Python, Rust, TypeScript/JavaScript, Go, Java, Kotlin, Swift, C#, Ruby.

One spec governs all of them. The same `basis.yaml` works whether your codebase is one language or nine.

## Install

```bash
cargo install basis-cli
```

## Quick Start

Create `basis.yaml` in your project root:

```yaml
governance:
  version: "1.0"

layers:
  domain:
    role: "Data types and business rules"
    packages: ["src/models", "src/types"]
    rules:
      purity: strict
    depends_on: []

  logic:
    role: "Application logic"
    packages: ["src/logic", "src/services"]
    depends_on: [domain]

  infra:
    role: "IO, networking, storage"
    packages: ["src/api", "src/storage"]
    depends_on: [domain, logic]

newtypes:
  enabled: true
  exclude_params: [index, count, offset]
  exclude_functions: [len, default]
  types:
    - name: UserId
      wraps: string
    - name: OrderId
      wraps: string

exhaustive_matching:
  enabled: true
  unions:
    - name: OrderStatus
      variants: [Pending, Confirmed, Shipped, Delivered, Cancelled]

purity:
  enabled: true
  forbidden_in_strict:
    - file_io
    - network_io
    - stdout
    - stderr
    - env_vars
    - system_clock
    - subprocess
```

Run:

```bash
basis-cli check --spec basis.yaml .
```

## Adopting Basis on an Existing Codebase

A blank `basis.yaml` is a blank-page problem. `basis-cli infer` solves it by reading what your code already does:

```bash
basis-cli infer . --output basis.yaml
```

It walks the source tree, treats top-level directories as layers, derives `depends_on` from real imports, marks layers with no IO as `purity: strict`, surfaces frequently-typed parameter names as newtype candidates, and detects switch/match statements that look like enum unions.

The output is a draft, not a destination. It describes the codebase as it is — drift and all. Rename layers to match how you actually think about the system. Drop newtype candidates that are coincidence. Add the constraints `infer` cannot know about.

Run `basis-cli infer` against this repo and you'll get a smaller, less opinionated spec than the one we ship — and that gap is the point. Inference recovers structure that is implicit in the code; it cannot recover the boundary rule that says "only `spec-loader` may touch the network," or the distinction between a `dictionary` and a `laboratory`. Architectural intent lives outside the source. Encoding it is a job for a human. Enforcing it is the job of Basis.

## Start Relaxed, Tighten Over Time

A spec that fails on day one gets disabled. Begin loose:

1. **Infer** the spec — it captures the structure that already exists.
2. **Run `basis-cli check`** — it should pass, or nearly so.
3. **Tighten one axis at a time.** Add a `boundaries` deny rule. Mark a layer `purity: strict` and fix the IO it surfaces. Promote a frequently-confused parameter to a newtype. Commit. Repeat.
4. **Use `basis-cli baseline` and `basis-cli trend`** to ratchet — block new violations without forcing a flag-day cleanup of existing ones.

The architecture you have is rarely the architecture you want. Basis lets you encode the gap and close it incrementally, instead of declaring bankruptcy.

## Self-Governance

Basis governs itself. The repo's own `basis.yaml` defines four layers (dictionary, laboratory, spec-loader, basis-runner) and enforces all four axes on every commit. 340 tests.

## Spec Reference

See [docs/governance-spec.md](docs/governance-spec.md) for the full spec format.

See [docs/guide.md](docs/guide.md) for a complete walkthrough.
