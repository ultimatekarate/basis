# Architectural Governance Specification

## The Problem

LLMs amplify whatever the developer already is. Good architecture plus LLM produces correct code at machine speed. Bad architecture plus LLM produces incorrect code at machine speed. The industry's current approach — reactive correction lists (`CLAUDE.md`, prompt rules, post-hoc review) — is O(n) in failure modes and does not scale.

The solution is proactive architectural governance that makes illegal states unrepresentable. When the decision space is constrained so that only correct outputs compile, the LLM cannot hallucinate on the constrained dimensions. Reliability becomes a property of the architecture, not the model.

## The Basis

Every architectural governance rule reduces to a constraint along one of four orthogonal axes:

| Axis | Governs | Makes Unrepresentable |
| ------ | --------- | ---------------------- |
| **Values** | Data representation | Invalid values — a `Did` cannot be confused with a `NetworkId` |
| **Placement** | Code location | Invalid imports — a forensics module cannot import IO |
| **Completeness** | Case handling | Unhandled states — every enum variant must be addressed |
| **Purity** | Side effects | Invalid effects — a pure function cannot perform IO |

These four axes are orthogonal (independent) and complete (every governance rule decomposes onto them). Solutions along each axis compose without interference.

### Why These Four Axes

A program does four things at the architectural level: it represents data, it organizes code into modules, it branches on state, and it causes effects in the world. Each of these can go wrong independently. Each requires its own constraint.

**Values** constrains data representation. When a function accepts a raw `string`, any string will do — a user ID, a file path, a SQL fragment. The compiler cannot distinguish them. Branded newtypes make confusion a type error. This axis answers: *is the data what it claims to be?*

**Placement** constrains visibility. When any module can import any other, dependency cycles form, layers collapse, and the codebase becomes a graph where everything depends on everything. Explicit layer boundaries make invalid imports uncompilable. This axis answers: *can this code see that code?*

**Completeness** constrains control flow. When a match statement handles three of four variants and hides behind a `default:` branch, the fourth variant is a silent failure waiting to happen. Exhaustive matching makes unhandled states a compile error. This axis answers: *are all cases accounted for?*

**Purity** constrains effects. When any function can perform IO, read the clock, or mutate global state, the only way to know what a function does is to read its implementation. Effect restrictions make forbidden side effects a compile error. This axis answers: *does this code touch the outside world?*

#### Orthogonality

These four axes are independent because each constrains a different dimension of a program's structure:

- **Values** operates on types — the signatures of functions.
- **Placement** operates on imports — the dependency graph between modules.
- **Completeness** operates on branches — the arms of match and switch statements.
- **Purity** operates on calls — the IO and effect APIs a function invokes.

You can tighten or relax any axis without affecting the others. Adding a branded newtype does not change which modules can import which. Marking a layer strict-purity does not affect whether its match statements are exhaustive. No two axes ever produce the same error for the same defect, because they examine different syntax.

#### Completeness of the Basis

Every architectural governance rule found in practice decomposes onto these four axes. Layered architecture, hexagonal ports and adapters, the dependency rule — all Placement. Branded types, value objects, newtypes — all Values. Exhaustive pattern matching, sealed class hierarchies — all Completeness. Effect systems, monadic IO, pure/impure separation — all Purity.

The four axes correspond to the four things a compiler can statically verify about code structure: type identity, module visibility, branch totality, and effect tracking. These are not arbitrary categories. They are the dimensions along which static analysis can make guarantees.

#### Standing on Shoulders

The ideas behind Basis did not appear from the ether. They were shaped by people who thought deeply about how software should be built — people whose work I return to, and whose influence runs through every axis.

Grace Hopper insisted that machines should speak our language, not the other way around. The compiler as a bridge between human intent and machine execution — that conviction is the reason Basis is a compiler, not a linter. If the machine can understand the architectural intent, it should enforce it. To my mind, there is no greater influence on software engineering than Read Admiral "Based" Grace Hopper.

Margaret Hamilton gave software engineering its name and proved that disciplined architecture saves lives. The layered rigor she brought to Apollo — where a software error could be fatal — is the same rigor Basis applies to every codebase. Architecture is not optional when failure is not optional. I think it is egregious that we do not have to clarify "Hamiltonian in what sense?" when discussing software.

David Parnas showed that what a module hides matters more than what it exposes. Information hiding and modular decomposition are the intellectual seed of the Placement axis. A module boundary is only meaningful if it can be enforced.

Alistair Cockburn drew a hexagon and changed how we think about inside and outside. Ports and adapters gave us a vocabulary for separating domain logic from infrastructure — a separation that Basis enforces through the combination of Placement and Purity.

Vint Cerf proved that layered contracts can hold a global network together. If the protocol stack can enforce that each layer speaks only to its neighbors, then a codebase can enforce the same. The internet works because layers are contracts, not suggestions.

Robert C. Martin articulated the dependency rule: dependencies point inward, always. Clean Architecture's lasting gift is the insight that the direction of a dependency is an architectural decision, and wrong directions compound. The Placement axis enforces this mechanically.

Basis is also Dijkstra-ian in spirit. The preference for structured constraints over undisciplined freedom — for making the space of possible programs smaller so that the remaining programs are more likely to be correct — is an influence absorbed rather than studied directly, but it runs through everything.

Fundamentally, Basis is a synthesis of all of these ideas.

## The Governance Spec

A single YAML file describes the architectural governance for any codebase in any language. The spec is read by per-language enforcement plugins (linters, type checker plugins, CI gates) and optionally by LLM tools to constrain code generation output.

### Format

```yaml
governance:
  version: "1.0"
  model: linguistic  # governance model identifier

# ── Layers ────────────────────────────────────────────────────────────

layers:
  dictionary:
    role: "Inert nouns. Data types, identifiers, configuration structures."
    packages:
      - "src/proto"
      - "src/models"
    rules:
      purity: strict          # no side effects permitted
      io: forbidden           # no IO imports
      network: forbidden      # no network imports
      database: forbidden     # no database imports
      async: forbidden        # no async/await
    depends_on: []             # depends on nothing

  laboratory:
    role: "Pure logic. Verification, analysis, transformation."
    packages:
      - "src/forensics"
      - "src/analysis"
    rules:
      purity: strict          # no side effects permitted
      io: forbidden           # no IO imports
      network: forbidden      # no network imports
      database: forbidden     # no database imports
      async: forbidden        # no async/await
    depends_on:
      - dictionary             # may import dictionary types

  hands:
    role: "Physical action. IO, networking, storage, user interaction."
    packages:
      - "src/node"
      - "src/transport"
      - "src/server"
    rules:
      purity: relaxed         # side effects permitted
      io: allowed
      network: allowed
      database: allowed
      async: allowed
    depends_on:
      - dictionary
      - laboratory

# ── Values Axis ───────────────────────────────────────────────────────

newtypes:
  enabled: true
  enforcement: strict          # newtypes must not be bypassed
  # Parameters and functions that are legitimately raw primitives.
  # exclude_params skips named parameters everywhere.
  # exclude_functions skips all parameters of named functions.
  # Individual declarations can also be suppressed with an inline
  # comment: // basis:allow(B002)
  exclude_params: [index, count, offset, size, length, capacity]
  exclude_functions: [len, to_string, default]
  types:
    # Define domain-specific newtypes that prevent primitive obsession.
    # Each newtype wraps a base type and optionally specifies validation.
    # Optional: languages: [python, js, ...] restricts the type to
    # specific languages. Omit for types shared across all languages.
    - name: Did
      wraps: string
      validation: "starts_with('did:key:')"
    - name: NetworkId
      wraps: string
    - name: RecordingId
      wraps: string
      validation: "uuid_v4"
    - name: CommunityId
      wraps: bytes
      validation: "length == 32"
    - name: PhalanxTimestamp
      wraps: u64

# ── Completeness Axis ─────────────────────────────────────────────────

exhaustive_matching:
  enabled: true
  # Tagged unions / discriminated unions / enums must be matched exhaustively.
  # Adding a new variant must cause a compile-time or lint-time error at
  # every consumption site.
  # Optional: languages: [python, js, ...] restricts the union to
  # specific languages. Omit for unions shared across all languages.
  unions:
    - name: NetworkEvent
      location: "dictionary"
      variants:
        - PeerDiscovered
        - GossipReceived
        - ShardResponseReceived
        - PeerDisconnected
        - Shutdown
    - name: PowerState
      location: "dictionary"
      variants:
        - Normal
        - Conserving
        - Leaf
        - Dormant
    - name: Evidence
      location: "dictionary"
      variants:
        - Video
        - Audio
        - Proximity

# ── Purity Axis ───────────────────────────────────────────────────────

purity:
  enabled: true
  # Functions in strict-purity layers must not:
  # - Perform IO (file, network, database, stdin/stdout)
  # - Access global mutable state
  # - Generate random numbers (except via injected RNG)
  # - Read environment variables
  # - Read system clock (except via injected timestamp)
  #
  # Enforcement mechanism varies by language:
  # - Rust: crate dependency graph (no tokio, no std::io in pure crates)
  # - Python: @pure decorator + mypy plugin
  # - TypeScript: eslint rule banning IO APIs in laboratory modules
  forbidden_in_strict:
    - "file_io"
    - "network_io"
    - "database"
    - "stdout"
    - "stderr"
    - "env_vars"
    - "system_clock"
    - "global_mutable_state"
    - "random"

# ── Placement Axis ────────────────────────────────────────────────────

boundaries:
  enabled: true
  # Cross-layer import rules. These are derived from the layer dependency
  # graph but can be made explicit for enforcement tools that don't
  # understand layer definitions.
  rules:
    - from: dictionary
      to: laboratory
      action: deny
      reason: "Dictionary cannot depend on Laboratory"
    - from: dictionary
      to: hands
      action: deny
      reason: "Dictionary cannot depend on Hands"
    - from: laboratory
      to: hands
      action: deny
      reason: "Laboratory cannot depend on Hands"
    - from: hands
      to: dictionary
      action: allow
    - from: hands
      to: laboratory
      action: allow
  # External dependency restrictions per layer.
  external:
    dictionary:
      allow: ["serde", "thiserror"]
      deny_patterns: ["tokio", "async-*", "reqwest", "sqlx", "std::io"]
    laboratory:
      allow: ["serde", "thiserror", "ed25519-dalek", "sha2"]
      deny_patterns: ["tokio", "async-*", "reqwest", "sqlx", "std::io"]
    hands:
      allow: ["*"]

# ── LLM Integration ──────────────────────────────────────────────────

llm:
  enabled: true
  # When an LLM tool reads this spec, it should:
  # 1. Classify each new type/function by linguistic role before writing code
  # 2. Place code only in packages matching the classified layer
  # 3. Enforce newtype usage at API boundaries (no raw primitives)
  # 4. Generate exhaustive match arms for all union variants
  # 5. Never import forbidden dependencies in strict-purity layers
  #
  # This section provides natural language guidance for LLM tools
  # that cannot parse the structured rules above.
  guidance: |
    This codebase follows the Linguistic Code Model:

    DICTIONARY (src/proto, src/models): Nouns. Inert data types.
    No IO, no async, no side effects. Types are defined here and
    nowhere else. If you are creating a new struct or enum that
    represents a domain concept, it belongs here.

    LABORATORY (src/forensics, src/analysis): Verbs. Pure logic.
    No IO, no async, no side effects. Verification, validation,
    transformation, analysis. All functions must be deterministic
    and testable without mocks. If you are writing logic that
    processes or analyzes data, it belongs here.

    HANDS (src/node, src/transport, src/server): Physical actions.
    IO, networking, storage, timers, platform APIs. Actor-based
    concurrency. No shared mutable state — use channels. If the
    code touches the outside world, it belongs here.

    Before writing any code, ask: Is this a noun? A verb? A physical
    action? The answer determines the layer. There is exactly one
    correct answer.

# ── Enforcement ───────────────────────────────────────────────────────

enforcement:
  # Per-language enforcement configuration.
  # Each language maps governance axes to concrete tooling.

  rust:
    values: "native newtypes + pub(crate) visibility"
    placement: "Cargo.toml dependency graph + pub(crate)"
    completeness: "native exhaustive match"
    purity: "crate dependency graph (no IO crates in pure layers)"
    tools: ["cargo check", "clippy"]

  python:
    values: "pydantic models + mypy plugin for branded types"
    placement: "ruff/pylint plugin reading governance spec"
    completeness: "mypy plugin for discriminated unions"
    purity: "@pure decorator + mypy plugin banning IO calls"
    tools: ["mypy", "ruff", "pytest"]

  typescript:
    values: "branded types + eslint plugin"
    placement: "eslint import restriction rules"
    completeness: "ts-pattern + eslint exhaustive switch rule"
    purity: "eslint rule banning IO APIs in laboratory modules"
    tools: ["tsc", "eslint", "vitest"]

  java:
    values: "record types + ArchUnit custom rule"
    placement: "ArchUnit layer dependency rules"
    completeness: "sealed class + ArchUnit switch check"
    purity: "ArchUnit rule banning IO classes in pure layers"
    tools: ["javac", "ArchUnit", "JUnit"]

  csharp:
    values: "record structs + Roslyn analyzer"
    placement: "Roslyn analyzer import rules"
    completeness: "native exhaustive switch + Roslyn analyzer"
    purity: "Roslyn analyzer banning IO in pure namespaces"
    tools: ["dotnet build", "Roslyn analyzers", "xUnit"]

  go:
    values: "defined types + golangci-lint custom linter"
    placement: "go-critic + depguard rules"
    completeness: "exhaustruct + exhaustive switch linter"
    purity: "custom golangci-lint rule banning IO in pure packages"
    tools: ["go vet", "golangci-lint", "go test"]

  kotlin:
    values: "value classes + detekt custom rule"
    placement: "detekt import restriction rules"
    completeness: "sealed when + detekt exhaustive check"
    purity: "detekt rule banning IO in pure packages"
    tools: ["kotlinc", "detekt", "JUnit"]

  swift:
    values: "typealiases + SwiftLint custom rule"
    placement: "SwiftLint import restriction rules"
    completeness: "native exhaustive switch"
    purity: "SwiftLint rule banning IO APIs in pure modules"
    tools: ["swiftc", "SwiftLint", "XCTest"]

# ── CI/CD Integration ─────────────────────────────────────────────────

ci:
  # Governance checks run as CI gates. PRs that violate governance
  # are blocked before review. This prevents architectural decay
  # at machine speed.
  gates:
    - name: "governance-check"
      description: "Verify all four governance axes"
      command: "governance-cli check --spec governance.yaml"
      blocking: true
    - name: "governance-report"
      description: "Generate governance health dashboard"
      command: "governance-cli report --spec governance.yaml --format json"
      blocking: false
```
