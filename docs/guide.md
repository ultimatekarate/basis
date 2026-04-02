# How to Use Basis

## Installation

```bash
cargo install basis-cli
```

Verify:

```bash
basis-cli --help
```

## Writing a Spec

A `basis.yaml` file describes your architecture. It has five sections, all optional except `governance`:

```yaml
governance:
  version: "1.0"
```

That's a valid spec. It doesn't enforce anything yet. You turn on what you need.

### Layers

Layers are the structural backbone. Each layer has a name, a role (documentation), a list of package paths, and a list of dependencies.

```yaml
layers:
  domain:
    role: "Data types and business rules"
    packages:
      - "src/models"
      - "src/types"
    depends_on: []

  logic:
    role: "Application logic and use cases"
    packages:
      - "src/logic"
      - "src/services"
    depends_on:
      - domain

  infra:
    role: "IO, networking, databases, external services"
    packages:
      - "src/api"
      - "src/storage"
      - "src/workers"
    depends_on:
      - domain
      - logic
```

The dependency graph is a DAG. Cycles are rejected at validation time. Dependencies are transitive — if `infra` depends on `logic` and `logic` depends on `domain`, then `infra` can import from `domain`.

The rule is simple: **a file can only import from layers it depends on.** A file in `src/models/` cannot import from `src/api/`. A file in `src/api/` can import from anywhere it depends on.

Layer names are yours to choose. There are no reserved names. Call them `domain`, `core`, `dictionary`, `adapters`, `controllers` — whatever fits your architecture.

### Purity

Mark layers as strict to forbid side effects:

```yaml
layers:
  domain:
    role: "Pure data types"
    packages: ["src/models"]
    rules:
      purity: strict
    depends_on: []
```

Then declare what's forbidden:

```yaml
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
    - process
    - dynamic_execution
```

In a `purity: strict` layer, Basis rejects imports and function calls associated with these categories. The categories are language-aware — `file_io` catches `open()` in Python, `std::fs` in Rust, `File(` in Java, `FileManager` in Swift.

Layers without `purity: strict` are unrestricted.

#### Per-Layer Overrides

The global `forbidden_in_strict` list applies to every strict layer. When different strict layers need different restrictions, use `per_layer` to override the global list for specific layers:

```yaml
purity:
  enabled: true
  forbidden_in_strict:
    - file_io
    - network_io
    - stdout
  per_layer:
    domain:
      also_forbid: [env_vars, system_clock]  # stricter than global
    renderer:
      allow: [stdout]                         # stdout OK in renderer
```

- `also_forbid` adds categories beyond the global list for that layer.
- `allow` exempts categories from the global list for that layer.

Both can be used together. The effective forbidden set for a layer is: `(global + also_forbid) - allow`.

`per_layer` only applies to layers marked `purity: strict`. Basis rejects `per_layer` entries that reference non-strict or nonexistent layers during validation.

### Newtypes (Values Axis)

Newtypes prevent primitive obsession. Define them with a canonical type:

```yaml
newtypes:
  enabled: true
  types:
    - name: UserId
      wraps: string
    - name: OrderId
      wraps: string
    - name: Amount
      wraps: float
    - name: EmailAddress
      wraps: string
      validation: email
```

Canonical types are `string`, `int`, `float`, `bool`. Basis maps these to each language's native primitive (`str` in Python, `String` in Rust, `string` in Go, etc.).

Basis scans public function signatures for parameters and return types whose name suggests a newtype but whose type is a raw primitive. A function `def get_user(user_id: str)` violates the spec because the parameter name contains `userid` and the type is `str` instead of `UserId`. A function `def get_user_id() -> str` violates the spec because the function name contains `user_id` and the return type is `str` instead of `UserId`.

Private functions are not checked. The boundary is at the public API.

The `validation` field is optional. It's used by `basis generate` to emit TODO stubs for validation logic. It has no effect on `basis check`.

#### Language Scoping

By default, every newtype applies to every language in the repo. In polyglot codebases, some types only make sense in one language. Add `languages:` to restrict a newtype to specific languages:

```yaml
newtypes:
  enabled: true
  types:
    # Shared across all languages
    - name: UserId
      wraps: string

    # TypeScript only — Electron window handle
    - name: WindowId
      wraps: int
      languages: [js]

    # Python only — backend API key
    - name: ApiKey
      wraps: string
      languages: [python]
```

When `languages:` is present, Basis only checks files in those languages for that newtype. `WindowId` scoped to `[js]` will not flag a Python function with a `window_id: str` parameter. `basis generate --lang python` will skip `WindowId` entirely.

When `languages:` is omitted, the newtype applies to all languages. Existing specs work without changes.

Valid language names: `python`, `rust`, `js`, `go`, `java`, `kotlin`, `swift`, `csharp`, `ruby`.

#### Suppressing B002

Not every raw primitive is primitive obsession. Parameters like `index`, `count`, `offset` are legitimately raw. Functions like `len` or `to_string` return primitives by nature. Basis provides three suppression mechanisms:

**1. `exclude_params`** — skip named parameters everywhere:

```yaml
newtypes:
  enabled: true
  exclude_params: [index, count, offset, size, length, capacity]
  types:
    - name: UserId
      wraps: string
```

**2. `exclude_functions`** — skip all parameters of named functions:

```yaml
newtypes:
  enabled: true
  exclude_functions: [len, to_string, default, capacity]
  types:
    - name: UserId
      wraps: string
```

**3. Inline `basis:allow(B002)`** — one-off override on a single declaration:

```python
# basis:allow(B002)
def lookup(user_id: str, index: int) -> User: ...
```

```rust
// basis:allow(B002)
pub fn lookup(user_id: &str, index: usize) -> User { ... }
```

The comment can appear on the declaration line or the line immediately above it. It works with any comment syntax (`#`, `//`, `--`, etc.).

The three mechanisms are applied in order: `exclude_params` and `exclude_functions` filter first, then inline comments. Use spec-level exclusions for systematic patterns and inline comments for one-off exceptions.

### Unions (Completeness Axis)

Declare the variants of a tagged union:

```yaml
exhaustive_matching:
  enabled: true
  unions:
    - name: OrderStatus
      variants:
        - Pending
        - Confirmed
        - Shipped
        - Delivered
        - Cancelled
    - name: PaymentMethod
      variants:
        - CreditCard
        - BankTransfer
        - Crypto
```

Basis scans match/switch/when/case statements for partial coverage. If a `switch` handles `Pending`, `Confirmed`, and `Shipped` but not `Delivered` or `Cancelled`, and there's no default/wildcard branch, Basis reports the missing variants.

A wildcard (`_`, `default`, `else`) suppresses the violation. Basis doesn't judge whether the wildcard is correct — only whether the cases are exhaustive.

#### Language Scoping

Like newtypes, unions can be scoped to specific languages:

```yaml
exhaustive_matching:
  enabled: true
  unions:
    # Matched in all languages
    - name: DocumentState
      variants: [Draft, Review, Published, Archived]

    # Python only — backend processing
    - name: TaskStatus
      languages: [python]
      variants: [Queued, Running, Completed, Failed, Cancelled]
```

When `languages:` is omitted, the union applies to all languages. When present, Basis only checks match statements in files of those languages. `basis generate` skips unions that don't apply to the target language.

### External Layers (Placement Axis)

Third-party packages are governed the same way as internal code — through layers and `depends_on`. Mark a layer as `external: true` to declare a group of external packages:

```yaml
layers:
  serialization:
    external: true
    role: "Serialization and deserialization"
    packages: [serde, thiserror]

  networking:
    external: true
    role: "Async runtime and HTTP"
    packages: [tokio, hyper, reqwest]

  domain:
    role: "Pure data types"
    packages: ["src/models"]
    rules: { purity: strict }
    depends_on: [serialization]

  infra:
    role: "IO and external services"
    packages: ["src/api", "src/storage"]
    depends_on: [domain, serialization, networking]
```

A file in `domain` can import `serde` (because `domain` depends on `serialization`) but cannot import `tokio` (because `domain` does not depend on `networking`). A file in `infra` can import both.

External layer packages use **segment-boundary matching** — the same algorithm as internal layer path resolution. An entry `E` matches import `I` if `I == E` (exact) or `I` starts with `E/` (sub-path). This prevents `serde` from matching `serde_json` — they're different crates.

| Entry | Matches | Doesn't match |
|---|---|---|
| `serde` | `serde`, `serde/Serialize` | `serde_json` |
| `tokio` | `tokio`, `tokio/runtime` | `tokio_util` |
| `net/http` | `net/http`, `net/http/handler` | `net/smtp` |

#### Wildcard External Access

For layers that need unrestricted external access, use a wildcard:

```yaml
layers:
  all-external:
    external: true
    role: "Unrestricted external access"
    packages: ["*"]

  infra:
    packages: ["src/api"]
    depends_on: [domain, all-external]
```

Layers that depend on `all-external` can import any third-party package. Layers that don't are restricted to their declared external dependencies — any import that matches the wildcard but isn't allowed will be flagged.

#### Language-Internal Imports

Languages have built-in import prefixes that are neither your code nor third-party packages — Rust's `crate::`, `std::`, `super::`, Go's standard library, etc. Basis does not hardcode knowledge of any language's internal imports. Instead, declare them as an external layer:

```yaml
layers:
  rust-internal:
    external: true
    role: "Rust language-internal imports"
    packages: [crate, super, self, std, core, alloc]
```

Every internal layer that uses these imports should depend on `rust-internal`. This keeps language-specific knowledge in the spec, not in the tool.

#### Relative Path Resolution

Imports using `./` or `../` are resolved against the importing file's directory before layer matching. `import { X } from "../types/types"` in `analysis/trends.ts` resolves to `types/types`, which correctly matches the `types` internal layer. This is generic filesystem path resolution — no language-specific knowledge is involved.

#### Opt-in Governance

Imports that don't match any layer — internal or external — pass silently. Basis only flags imports that positively match a declared external layer the importing layer doesn't depend on. This means you can introduce external layers incrementally without triggering violations for packages you haven't categorized yet.

#### External Layer Transitivity

Dependencies between external layers propagate transitively:

```yaml
layers:
  networking:
    external: true
    packages: [tokio, hyper]

  web-framework:
    external: true
    packages: [actix-web, actix-rt]
    depends_on: [networking]

  api-layer:
    packages: ["src/api"]
    depends_on: [web-framework]
```

`api-layer` can import `actix-web` (direct) and `tokio` (transitive through `web-framework`). The relationship is expressed once, not repeated in every consumer.

#### External Layer Rules

- External layers' `packages` are package names, not file paths. They're matched against imports, not used for file resolution.
- External layers cannot have `rules` (purity settings) — they describe code you don't control.
- External layers can have `depends_on` (for transitivity) and `role` (documentation).
- The dependency graph (including external layers) must be a DAG.
- `basis check` does not walk files in external layers — they have no source files.

### Boundary Rules

Boundaries are usually implied by `depends_on`, but you can add explicit structural overrides:

```yaml
boundaries:
  enabled: true
  rules:
    - from: domain
      to: infra
      action: deny
      reason: "Domain must not depend on infrastructure"
```

## Inferring a Spec

If you have an existing codebase and don't want to write a spec from scratch, `basis infer` can generate a draft for you:

```bash
basis-cli infer . --output basis.yaml
```

This walks your codebase, analyzes its structure, and produces a `basis.yaml` that describes the architecture as-is. The output includes:

- **Layers** inferred from directory structure, with roles guessed from directory names (`models` -> "Data definitions", `api` -> "API/IO boundary", etc.)
- **Newtypes** inferred from recurring parameter name patterns (`user_id: str` appearing across multiple files suggests a `UserId` newtype wrapping `string`)
- **Unions** inferred from match/switch statements that share overlapping case sets
- **Purity** classification based on which layers contain IO operations and which don't
- **Boundaries** derived from the actual import graph between inferred layers

### Options

```bash
# Write to file (default is stdout)
basis-cli infer . --output basis.yaml

# Require more occurrences before proposing a newtype (reduces noise)
basis-cli infer . --min-occurrences 3

# Show reasoning for each inference decision
basis-cli infer . --verbose
```

### The inference-to-spec gap

Running `basis check` against an inferred spec should produce near-zero violations — because the spec describes what the code already does. The value comes from what you do next: **tighten it** — and that is a human job. Basis will never automate this step.

The inferred spec is a photograph of your architecture. The final spec is a blueprint. The gap between them is precisely where architectural intent lives. Closing that gap requires judgment that no tool can provide:

- **Layer grouping** — inference sees one layer per directory. You see that `validate/` and `report/` are conceptually the same "pure logic" layer. That decision exists nowhere in the code.
- **Meaningful newtypes** — inference finds every recurring `_id` parameter. You decide that `UserId` is a domain concept worth enforcing but `Name` is just a common variable name. That distinction is domain knowledge.
- **Semantic unions** — inference can only find unions that already appear as match arms. You define unions that *should* exist — the complete set of states, the taxonomy of axes — as prescriptive constraints.
- **External layers** — inference sees third-party imports but doesn't group them into external layers. You decide that `serde` and `thiserror` belong to a `serialization` layer, that `tokio` and `hyper` belong to `networking`, and that your domain layer should only access `serialization`. That grouping is architectural intent — it doesn't exist in the import graph.
- **Negative constraints** — inference observes what *is*. You specify what *must not be* — "this layer must never do IO, even though nothing stops it today." There is no code evidence for a rule that has never been violated.

If `basis infer` could perfectly reproduce your spec, the spec would contain no information the code doesn't already have. The fact that it can't is the proof that the spec carries real architectural intent.

The three roles are distinct: `basis infer` generates the scaffold, the architect applies intent, `basis check` enforces the result. Tightening the spec is the architect's irreducible contribution — the part that makes governance meaningful rather than mechanical.

### Recommended workflow

1. Run `basis infer . --output basis.yaml`
2. Run `basis check --spec basis.yaml .` to verify near-zero violations
3. **You** edit `basis.yaml`: merge layers by intent, remove noisy newtypes, add domain-specific unions, add negative constraints
4. Run `basis check` again — violations appear where reality doesn't match your intent
5. Fix the violations or adjust the spec
6. Commit `basis.yaml` and add `basis check` to CI

## Running Basis

### Check everything

```bash
basis-cli check --spec basis.yaml .
```

Walks all source files in the current directory, detects their language by extension, and checks all four axes. Returns exit code 0 on success, 1 on violations.

### Check one axis

```bash
basis-cli check --spec basis.yaml --axes values .
basis-cli check --spec basis.yaml --axes placement .
basis-cli check --spec basis.yaml --axes completeness .
basis-cli check --spec basis.yaml --axes purity .
```

### Check a subdirectory

```bash
basis-cli check --spec basis.yaml src/
```

### Validate the spec itself

```bash
basis-cli validate basis.yaml
```

Checks for duplicate newtypes, duplicate union names, empty variant lists, unknown layer dependencies, and dependency cycles.

### Generate code skeletons

```bash
basis-cli generate --lang <python|rust|js|go|java|kotlin|swift|csharp|ruby> --spec basis.yaml --output generated/
```

Produces a types file with newtype definitions, union declarations, and exhaustive match scaffolds in that language's idiom. The generated code passes `basis check` by construction.

### Generate a report

```bash
basis-cli report --spec basis.yaml
basis-cli report --spec basis.yaml --format json
```

### Structured output

```bash
basis-cli check --spec basis.yaml --format json .
```

Outputs every violation as structured JSON to stdout. Each violation includes a stable identity key (file + function + type, excluding line numbers) for reliable diffing across runs.

## Measuring Technical Debt

Technical debt is usually a feeling. Basis makes it a number.

**Snapshot your current violations:**

```bash
basis-cli baseline --spec basis.yaml .
# Baseline saved: 47 violations to .basis-baseline.json
```

**Later, see what changed:**

```bash
basis-cli trend --spec basis.yaml .
```

```
Basis Trend: comparing against .basis-baseline.json

  Improved:  -12 violations (3 values, 5 placement, 4 purity)
  Regressed: +2 violations (2 completeness)
  Unchanged: 33 violations

  Net change: -10 violations
```

The four axes tell you *what kind* of debt you have. Placement debt means your boundaries are leaking. Values debt means your types are confused. Completeness debt means you have unhandled cases. Purity debt means side effects are in the wrong places. Different problems, different fixes.

### Gating CI on regression

```bash
# Strict: zero new violations allowed
basis-cli trend --spec basis.yaml --fail-on-regression .

# Lenient: fail only if total count went up
basis-cli trend --spec basis.yaml --fail-on-net-regression .
```

Both exit 1 on failure. Use `--fail-on-net-regression` for teams actively paying down debt — it allows new violations as long as you fixed more than you introduced.

### Offline mode

For CI pipelines where the check already ran:

```bash
basis-cli check --spec basis.yaml --format json . > current.json
basis-cli trend --current current.json --baseline .basis-baseline.json --spec basis.yaml .
```

### JSON output for dashboards

```bash
basis-cli trend --spec basis.yaml --format json .
```

Returns `improved`, `regressed`, and `unchanged` arrays with full violation details.

### Known limitations

File renames cause the old path to appear as "improved" and the new path as "regressed." Re-run `basis baseline` after bulk renames.

## Error Codes

| Code | Axis | Meaning | Fix |
|------|------|---------|-----|
| B001 | Placement | Import crosses layer boundary | Move code to correct layer or update `depends_on` |
| B002 | Values | Raw primitive parameter or return type where newtype required | Use the branded newtype (`UserId` not `str`), or suppress with `exclude_params`/`exclude_functions`/`basis:allow(B002)` |
| B003 | Completeness | Non-exhaustive match/switch | Add missing case arms or a wildcard |
| B004 | Purity | Forbidden import or call in strict layer | Move IO code to a non-strict layer |

Every error includes a `help:` line that tells you exactly what to do. Read it.

## Integrating with CI

### GitHub Actions

```yaml
name: Basis
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Basis
        run: cargo install basis-cli
      - name: Check architecture
        run: basis-cli check --spec basis.yaml .
```

For teams adopting Basis incrementally on an existing codebase, use trend-based gating instead of zero-violation enforcement:

```yaml
name: Basis Trend
on: [push, pull_request]
jobs:
  trend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Basis
        run: cargo install basis-cli
      - name: Check for regressions
        run: basis-cli trend --spec basis.yaml --fail-on-net-regression .
```

This allows existing violations but blocks any commit that increases the total count. Commit `.basis-baseline.json` to the repo and update it periodically as the team pays down debt.

### Pre-commit hook

Add to `.pre-commit-config.yaml`:

```yaml
- id: basis-check
  name: basis check
  description: Compiler-enforced architectural governance
  entry: basis-cli check
  language: system
  types_or: [python, rust, javascript, typescript, java, kotlin, go, swift, csharp, ruby]
```

### Claude Code

Add this to your project's `CLAUDE.md`:

```markdown
## Architecture

This project uses Basis for architectural governance. The spec is `basis.yaml`.

### Rules

1. **Run `basis-cli check --spec basis.yaml .` after writing or modifying code.** Violations are errors, not warnings. Fix them before moving on.
2. **Read the `help:` line** in each error — it tells you exactly what to do.
3. **Never work around a violation.** Do not suppress checks, add exceptions, or restructure code to avoid detection. Fix the root cause.

### Error codes

| Code | Axis | Fix |
|------|------|-----|
| `B001` | Placement | Move code to correct layer or update `depends_on` |
| `B002` | Values | Use the branded newtype (`UserId` not `str`) |
| `B003` | Completeness | Add missing case arms or wildcard |
| `B004` | Purity | Move IO code to a non-strict layer |

### Layer placement

Before writing code, determine which layer it belongs in by reading the
`role` field of each layer in `basis.yaml`. Place the file in the correct
package directory. If unsure, check `depends_on` — a file cannot import
from a layer it does not depend on.
```

That's it. Claude reads `CLAUDE.md` at the start of every session, runs `basis check` after writing code, reads the error and `help:` line, fixes the violation, and re-checks. The loop is typically 1-2 iterations.

The key difference from other governance approaches: most catch violations at review time, when the code is already written. CI catches them at commit time. Basis with Claude Code catches them at write-time — violations are rejected before the code exists.

#### What Claude sees

When Claude writes a function `def get_user(user_id: str)` in a codebase where `UserId` wraps `string`:

```
error[B002]: parameter 'user_id' uses raw 'str' instead of branded newtype
  --> src/api/routes.py:14
  = help: use UserId instead of str
```

Claude reads the `help:` line, changes `str` to `UserId`, and re-runs the check. No ambiguity, no judgment call — the error tells Claude exactly what to do.

#### Why this works

LLMs are good at logic. They're bad at discipline. They'll write brilliant algorithms and then import `requests` in your domain layer, use a raw `str` where you defined `UserId`, or handle 4 of 5 enum variants. These are exactly the mistakes Basis catches — structural violations that the LLM doesn't notice because the spec isn't in its attention window at line 47.

The 50ms check time means the feedback loop is instant. Write, check, fix, check. No waiting.

## Integrating with Other LLMs

The Claude Code pattern above works with any LLM tool that can run shell commands and read output. The integration is always the same: tell the LLM to run `basis-cli check --spec basis.yaml .` after writing code, read the error, fix the violation, re-check. The `help:` line in each error is written for machines as much as humans.

## Cross-Language Governance

The spec is language-agnostic. `wraps: string` means `str` in Python, `String` in Rust, `string` in Go and TypeScript. The CLI maps the four axes to each language's idioms automatically.

One `basis.yaml` can govern a polyglot repo. A Python service and a Rust library can share the same newtypes, the same unions, the same layer boundaries. `basis generate` produces the types in both languages. `basis check` verifies both. The architecture is provably isomorphic.

In polyglot repos, some types are shared and some are language-specific. Use `languages:` on newtypes and unions to control scope:

```yaml
newtypes:
  enabled: true
  types:
    - name: UserId            # all languages
      wraps: string
    - name: WindowId          # TypeScript only
      wraps: int
      languages: [js]
    - name: ApiKey             # Python only
      wraps: string
      languages: [python]
```

Types without `languages:` apply everywhere. Types with `languages:` are checked and generated only for those languages. This prevents false positives — a Python backend won't be flagged for not using `WindowId`, and a TypeScript frontend won't be flagged for not using `ApiKey`.

## Choosing an Architecture Model

Basis doesn't prescribe a model. The spec is a DAG of layers with purity rules. You choose the architecture:

**Hexagonal (Ports & Adapters):**

```yaml
layers:
  domain:
    packages: ["src/domain"]
    rules: { purity: strict }
    depends_on: []
  ports:
    packages: ["src/ports"]
    rules: { purity: strict }
    depends_on: [domain]
  adapters:
    packages: ["src/adapters"]
    depends_on: [domain, ports]
```

**Clean Architecture:**

```yaml
layers:
  entities:
    packages: ["src/entities"]
    rules: { purity: strict }
    depends_on: []
  use-cases:
    packages: ["src/use_cases"]
    rules: { purity: strict }
    depends_on: [entities]
  interface-adapters:
    packages: ["src/adapters"]
    depends_on: [entities, use-cases]
  frameworks:
    packages: ["src/frameworks"]
    depends_on: [entities, use-cases, interface-adapters]
```

**Linguistic Code Model (functional core / imperative shell):**

```yaml
layers:
  dictionary:
    packages: ["src/models"]
    rules: { purity: strict }
    depends_on: []
  laboratory:
    packages: ["src/logic"]
    rules: { purity: strict }
    depends_on: [dictionary]
  hands:
    packages: ["src/api", "src/storage"]
    depends_on: [dictionary, laboratory]
```

**Flat (no layers, just types and exhaustiveness):**

```yaml
governance:
  version: "1.0"
newtypes:
  enabled: true
  types:
    - name: UserId
      wraps: string
exhaustive_matching:
  enabled: true
  unions:
    - name: OrderStatus
      variants: [Pending, Confirmed, Shipped]
```

Every axis is optional. Use what you need.

## What Basis Doesn't Do

- **Type checking** — Basis doesn't verify that your code compiles. Use your language's compiler for that.
- **Logic verification** — Basis doesn't check that your algorithm is correct. It checks that your algorithm is in the right place, uses the right types, and handles all cases.
- **Deep purity analysis** — Basis checks for direct imports and calls to IO functions. It doesn't trace through call chains to find transitive impurity. A function that calls a function that calls `open()` won't be caught unless it directly imports or calls an IO function itself.
- **Runtime enforcement** — Basis is a static check. It runs on source code, not on running programs.
- **Ruby return type checking** — Ruby's Sorbet `sig` blocks use `.returns(Type)` syntax which Basis does not currently parse for return type violations. Parameter checking via Sorbet `params()` works. All other languages with static type annotations (Python, Rust, TypeScript, Go, Java, Kotlin, Swift, C#) check both parameters and return types.

These are deliberate constraints. Basis is a 50ms string scanner, not a type system. It trades depth for breadth — nine languages, four axes, zero dependencies on language-specific tooling.

## Test File Exclusion

Basis automatically excludes test files from checking. Each language has its own conventions:

- Python: files in `tests/` or `__tests__/`, files starting with `test_` or ending with `_test.py`
- Rust: files in `tests/`, files ending with `_test.rs`
- Go: files ending with `_test.go`
- Java/Kotlin: files in `test/`, files ending with `Test.java` or `Tests.java`
- And so on for each language

Test code can import anything, use raw primitives, and skip enum arms. The governance applies to production code.

## Why It Works: A Case Study

During development of Basis itself, we implemented spec composition — the `extends:` feature that lets a child spec inherit from a parent. The feature required code in three governed layers:

- **dictionary** (spec.rs, strict purity) — the `extends: Option<String>` field on `BasisSpec`
- **laboratory** (validate.rs, strict purity) — `merge_specs()`, a pure function that takes two specs and returns one
- **spec-loader** (loader.rs, IO allowed) — `load_spec_with_chain()`, which reads parent files and detects circular extends chains

The implementation compiled, passed all tests, and `basis check` reported zero violations on the first run.

Basis didn't catch a single mistake.

This sounds like a failure. It isn't. Here's what actually happened: after reading `basis.yaml` — four layers, dependency arrows, purity rules — it was immediately obvious where every function belonged. The merge logic is pure data transformation, so it goes in laboratory. File reading is IO, so it goes in the loader. The data type is inert, so it goes in dictionary. There was no ambiguity, no judgment call, no "it could go either way."

The spec didn't need to catch violations because it made the architecture so legible that violations didn't occur.

This is the deeper argument for Basis. A `basis.yaml` is not just a set of constraints — it's a communication protocol between the architect and every future contributor. It says: here are the layers, here's what's pure, here's what's allowed to do IO, here are the types that must not be confused, here are the unions that must be handled exhaustively. After reading that, the design space collapses. There is usually only one right place for any given piece of code.

Most codebases encode their architecture in convention, tribal knowledge, and stale documentation. A new contributor (human or AI) has to read dozens of files to infer the patterns, and even then they're guessing. With Basis, they read one file and the structural intent is unambiguous — because it's enforced, it can't have drifted from reality.

Basis works at two levels:

1. **Active enforcement** — catching violations when someone puts code in the wrong place, uses a raw primitive, misses an enum arm, or imports IO in a pure layer
2. **Passive clarity** — making the architecture so explicit that most violations never happen in the first place

The first level is what CI catches. The second level is what makes LLMs (and new team members) productive on day one.

## FAQ

**Can I use Basis without layers?**
Yes. Every section except `governance` is optional. You can use just newtypes, just exhaustive matching, just purity, or any combination.

**Can I have more than three layers?**
Yes. There's no limit. Use as many layers as your architecture needs.

**Can two layers depend on each other?**
No. The dependency graph must be acyclic. `basis validate` rejects cycles.

**Does Basis modify my code?**
No. `basis check` is read-only. `basis generate` writes new files to an output directory.

**How does Basis detect languages?**
By file extension. `.py` is Python, `.rs` is Rust, `.ts` and `.tsx` are TypeScript, `.go` is Go, `.java` is Java, `.kt` is Kotlin, `.swift` is Swift, `.cs` is C#, `.rb` is Ruby.

**What if my codebase uses a language Basis doesn't support?**
Files with unrecognized extensions are skipped. Basis checks what it can and ignores the rest.

**Is Basis fast enough for a pre-commit hook?**
Yes. It checks ~37 files in 50ms. It would check 3,700 files in about 5 seconds, dominated by filesystem IO. There is no parsing step — just string matching.

**Does Basis need my code to compile first?**
No. Basis reads source files as text. It doesn't invoke any compiler, interpreter, or build tool.
