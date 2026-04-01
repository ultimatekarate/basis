# Basis

Compiler-enforced architectural governance. One YAML file. Nine languages. 50ms.

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

## What Basis Does

You define your architecture in `basis.yaml`. Basis rejects any code that violates it.

Four axes, each independent:

| Axis | Governs | Error |
| ------ | --------- | ------- |
| **Values** | Data representation — `UserId` cannot be confused with `OrderId` | B002 |
| **Placement** | Code location — a data module cannot import IO | B001 |
| **Completeness** | Case handling — every union variant must be addressed | B003 |
| **Purity** | Side effects — a pure layer cannot perform IO | B004 |

## Basis Is a Compiler

Not a linter. Violations are errors, not warnings. `basis check` returns a non-zero exit code. CI blocks the merge. The architecture cannot decay.

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

## Self-Governance

Basis governs itself. The repo's own `basis.yaml` defines four layers (dictionary, laboratory, spec-loader, basis-runner) and checks pass on every commit. 4,227 lines of production code, 2,900 lines of tests, 223 tests total.

## Spec Reference

See [docs/governance-spec.md](docs/governance-spec.md) for the full spec format.

See [docs/guide.md](docs/guide.md) for a complete walkthrough.
