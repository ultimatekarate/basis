# Basis — Project Conventions

## What Is Basis

Compiler-enforced architectural governance. A single `basis.yaml` spec constrains code generation along four orthogonal axes: Values, Placement, Completeness, Purity.

## Repository Layout

```
basis-cli/                 Rust CLI — validates specs, checks codebases
docs/                      Spec reference, quickstarts
examples/                  Example basis.yaml specs
basis.yaml                 Basis governs itself
```

## Naming

- Product name: **Basis** (not "governance")
- CLI: `basis-cli`
- Spec file: `basis.yaml`

## The Four Axes

| Axis | Governs | Enforced By |
| --- | --- | --- |
| **Values** | Data representation (branded newtypes) | CLI |
| **Placement** | Code location (import boundaries) | CLI |
| **Completeness** | Case handling (exhaustive matching) | CLI |
| **Purity** | Side effects (IO restrictions) | CLI |

## Basis Is a Compiler

Basis is **not a linter**. It is a compiler. `basis check` violations are errors, not warnings.

When writing or modifying code in this repository:

1. **All code must pass `basis check`** before it is considered complete. Violations block progress the same way `cargo check` rejects invalid Rust.
2. **Fix violations immediately** — do not defer, suppress, or ignore them. A violation means the code is wrong.
3. **Error codes are stable and actionable:**

| Code | Axis | Meaning | Fix |
|------|------|---------|-----|
| `B001` | Placement | Import crosses layer boundary | Move code to correct layer or update `depends_on` |
| `B002` | Values | Raw primitive where newtype required | Use the branded newtype (e.g., `UserId` not `str`) |
| `B003` | Completeness | Non-exhaustive match/switch | Add missing case arms or wildcard |
| `B004` | Purity | Forbidden import in strict layer | Move IO code to a non-strict layer |

4. **Read the `help:` line** in each error — it tells you exactly what to do.
5. **Never work around a violation** by disabling checks, adding exceptions, or restructuring code to avoid detection. Fix the root cause.

## Coding Standards

- Rust: stable toolchain, `cargo fmt`, `cargo clippy`
- All tools read the same `basis.yaml` — no tool-specific config files
