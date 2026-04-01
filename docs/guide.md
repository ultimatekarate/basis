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

### Boundaries (Placement Axis)

Boundaries are usually implied by `depends_on`, but you can add explicit rules:

```yaml
boundaries:
  enabled: true
  rules:
    - from: domain
      to: infra
      action: deny
      reason: "Domain must not depend on infrastructure"
  external:
    domain:
      allow: ["serde", "thiserror"]
      deny_patterns: ["tokio", "std::io", "reqwest"]
    infra:
      allow: ["*"]
```

The `external` section restricts third-party imports per layer. A `domain` layer that imports `tokio` is a violation.

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

Basis works with Claude Code out of the box. When Claude writes code that violates `basis.yaml`, the error appears immediately. Claude reads the `help:` line, fixes the code, and moves on. The architecture enforces itself at write-time.

This is the key difference: most governance catches violations at review time, when the code is already written. CI catches them at commit time. Basis with Claude Code catches them at write-time — violations are rejected before the code exists.

## Integrating with LLMs

Basis is designed as a plug-in for LLM-assisted development. The workflow:

1. **Define the spec** — `basis.yaml` describes the architecture
2. **Generate skeletons** — `basis generate` produces type-safe scaffolds
3. **LLM fills logic** — the LLM writes implementation inside the scaffolds
4. **Basis verifies** — `basis check` rejects anything structurally wrong
5. **LLM fixes** — the LLM reads the error, reads the `help:` line, corrects the code
6. **Loop until clean** — typically 1-2 iterations

LLMs are good at logic. They're bad at discipline. They'll write brilliant algorithms and then import `requests` in your domain layer, use a raw `str` where you defined `UserId`, or handle 4 of 5 enum variants. These are exactly the mistakes Basis catches — structural violations that the LLM doesn't notice because the spec isn't in its attention window at line 47.

The 50ms check time means the feedback loop is instant. Generate, check, fix, check. No waiting.

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
