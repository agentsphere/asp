# About This Document

This is the [arc42](https://arc42.org/) architecture documentation for the **Platform** — a unified AI-first DevOps platform built as a single Rust binary.

## How to Read

The 12 sections follow the arc42 template:

| Section | What You'll Find |
|---|---|
| [01 Introduction & Goals](01-introduction-goals.md) | Business context, quality goals, stakeholders |
| [02 Constraints](02-constraints.md) | Technical, organizational, and convention constraints |
| [03 Context & Scope](03-context-scope.md) | System boundary and external interfaces |
| [04 Solution Strategy](04-solution-strategy.md) | Fundamental architectural decisions |
| [05 Building Blocks](05-building-blocks.md) | Static decomposition (modules and their APIs) |
| [06 Runtime View](06-runtime-view.md) | Key scenarios as sequence/flow diagrams |
| [07 Deployment View](07-deployment-view.md) | Infrastructure topology and K8s mapping |
| [08 Crosscutting Concepts](08-crosscutting-concepts.md) | Auth, RBAC, security, observability, testing |
| [09 Architecture Decisions](09-architecture-decisions.md) | ADRs for the 14 key decisions |
| [10 Quality Requirements](10-quality-requirements.md) | Quality tree with measurable scenarios |
| [11 Risks & Technical Debt](11-risks-technical-debt.md) | Known risks, gaps, and debt items |
| [12 Glossary](12-glossary.md) | Domain and technical terminology |

## Diagrams

All diagrams use [Mermaid.js](https://mermaid.js.org/) and render natively on GitHub.

**Source of truth**: the `.mmd` files in [`diagrams/`](diagrams/). A pre-commit hook (`hack/inject-mermaid.sh`) auto-injects their contents into the markdown files between marker comments:

    <!-- mermaid:diagrams/example.mmd -->
    ```mermaid
    (auto-injected from .mmd file)
    ```
    <!-- /mermaid -->

To update a diagram: edit the `.mmd` file, then commit. The hook handles the rest.

## Keeping This Up to Date

- **Diagrams**: edit `.mmd` files only; pre-commit hook syncs to markdown
- **Code-derived sections** (building blocks, state machines, ER diagram) can be regenerated from source
- **Narrative sections** (strategy, ADRs, crosscutting concepts) require manual updates when architecture changes
- **Runtime views** should be updated when major flows change

## Source of Truth

This documentation describes the system as-built. For coding conventions and implementation patterns, see [`CLAUDE.md`](../../CLAUDE.md). For the full database schema, see the `migrations/` directory.
