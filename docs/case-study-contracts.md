# Case Study: The Fifth Axis That Wasn't

## The Hypothesis

Basis governs code along four axes: Placement, Values, Completeness, Purity. Each checks a structural property of source code — what imports what, what types are used where, whether case arms are exhaustive, whether pure layers touch IO.

A fifth axis was proposed: **Contracts**. Inspired by Eiffel's Design by Contract, it would check that public functions have precondition guards referencing their parameters. The error code was B005. The spec entry looked like this:

```yaml
contracts:
  enabled: true
  preconditions:
    scope: public
    must_reference: parameters
  per_layer:
    spec-loader:
      preconditions:
        scope: public
        must_reference: parameters
```

The scanner would detect guard patterns — `assert`, `if/throw`, `if/return Err`, `debug_assert!` — in the first 10 lines of a function body, then check which parameters appeared in those guard lines.

## The Implementation

Contracts was fully implemented:

- **Spec types**: `ContractConfig`, `PreconditionConfig`, `LayerContractOverride`, with scope levels (`public`, `all`, `none`, `constructors`) and per-layer overrides.
- **Language scanners**: Full implementations for Python, Rust, JavaScript, and Go. Stub scanners for Java, Kotlin, Ruby, Swift, C#. Each detected language-idiomatic guard patterns.
- **Check module**: `check/contracts.rs` with file walking, layer resolution, scope filtering, and violation reporting.
- **Wiring**: Integrated into CLI (text and JSON output), LSP diagnostics, spec validation, and spec merging.
- **Dogfooding**: Basis governed itself with contracts. The self-check initially found 67 violations, which were resolved by setting the global scope to `none` (Rust's type system handles most preconditions) with per-layer overrides for `spec-loader` and `laboratory`.

The implementation touched 25 files and added 12 tests. It worked.

## What It Found

When contracts was applied to a JavaScript budget-tracker example, it caught a real gap: `createTransaction` and `handleAddTransaction` both accepted a `description` parameter without any guard. The code treated it as optional (`description || ""`), but never validated it. Someone could pass `42` as a description and it would silently become truthy, skip the fallback, and store a number in a string field.

This was a legitimate bug found by a legitimate check.

## Why It Was Removed

The other four axes share a property that contracts does not: **they cannot be satisfied by meaningless code**.

- A missing case arm is a missing case arm. You cannot game Completeness.
- A raw `string` where `UserId` is required is wrong. You cannot game Values.
- An import crossing a layer boundary is a violation. You cannot game Placement.
- An `import fs` in a strict-purity layer is forbidden. You cannot game Purity.

Contracts can be satisfied by `if (true) throw new Error("x")`. The scanner checks for guard *presence*, not guard *correctness*. A function with a meaningless guard passes. A function with no guard fails. The check produces vacuous truths — it verifies that certain lines of code exist, not that they do anything useful.

The other four axes check **structural relationships** between code elements. Contracts checks **local patterns** inside function bodies. It answers a different kind of question, and the answer it gives is not reliable enough for a compiler.

Basis is a compiler. Its violations are errors, not warnings. A vacuous truth is not a compiler error.

## The Removal

Contracts was removed in a single pass. The architecture that made it easy to add — self-contained check module, isolated scanner type, independent `From` impl — made it equally easy to remove. The deletion touched the same 25 files, removed the same 12 tests, and left the four remaining axes completely unaffected.

The budget-tracker example kept its guard clauses. They're good code. They're just not spec-enforced, because spec enforcement requires closed-form checks that can't be gamed.

## The Lesson

Not every checkable property belongs in a governance spec. The test is not "can we detect this?" but "can the detection be vacuously satisfied?" If a check can be passed by adding noise, it's a suggestion engine, not a compiler. Basis is a compiler.

The removal itself revealed a second lesson: good architectural discipline makes features easy to delete, not just easy to add. Most people measure architecture by how painlessly they can build new things. But the real test is whether you can change your mind without paying for it. A system that resists deletion is a system where every decision is permanent — and permanent decisions made under uncertainty are how you get legacy code. Contracts touched 25 files on the way in and the same 25 files on the way out. No scar tissue. The four remaining axes didn't notice.

Four axes. Four error codes. The number is not arbitrary — it is the number of behavioral error categories that are both statically checkable without executing the code and closed-form enough that satisfaction is meaningful.
