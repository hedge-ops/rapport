# Rapport Specifications

This directory contains the canonical product requirements for Rapport.

## Product Philosophy

Rapport provides an opinionated golden path for developing software: repository-
owned policy, explicit Context, frequent Git checkpoints, fast local builds,
independent review, proof bound to exact commits, and a conventional path to
the target branch.

If a developer works this way, Rapport should make the workflow feel effortless.
It favors convention over configuration and one excellent default over support
for every possible process. Rapport is not a generic DAG engine, ticket tracker,
build system, or infinitely configurable pipeline framework.

Rapport automates mechanics so the human can stay focused on intent, code, and
risk. An agent may accept corrective findings and perform more work; dismissing
a finding or overriding quality policy requires human direction. The pull
request puts the exact accepted candidate and its evidence in front of the
human to read without adding a duplicate approval solely to record that reading.

Rapport proof is a traceability and consistency mechanism under a trusted local
operator and agent host. It prevents accidental reuse of evidence for the wrong
candidate or policy; it is not a tamper-resistant security attestation.

## Lifecycle

```text
Plan -> Develop -> Build -> Review -> Integrate -> Ship
```

Plan turns intent into a durable request. Develop performs and records work
until there is one exact committed candidate. Build obtains current executable
proof for that candidate. Review independently evaluates the candidate against
its intent and effective repository policy. Integrate publishes the accepted
candidate, creates its pull request, verifies required GitHub build signoffs,
and moves it onto the target branch. Ship delivers it until the change is live.

Work is the durable local ledger spanning Develop, Build, Review, and Integrate.
It is not a separate lifecycle phase.

Plan and Ship are roadmap phases. The current specifications focus on Rules,
Context, Work, Build, Review, and Integrate.

## Requirement Shape

Each requirement is one meaningful developer journey. Supporting mechanics and
invariants belong inside that journey's scenarios.

Files use stable `[CATEGORY]-[INDEX].md` IDs:

- `RUL`: create, reuse, apply, and inspect repository standards.
- `CTX`: describe repository areas and declare their development policy.
- `WRK`: start, perform, checkpoint, and prepare development work.
- `BLD`: obtain build feedback and build acceptance proof.
- `REV`: obtain review feedback and independent acceptance.
- `INT`: move accepted work onto the target branch.

Implementation status and unresolved decisions belong in
[GAPS.md](./GAPS.md).

## Index

### Rules

- [RUL-001](./RUL-001.md) — Create and reuse shared repository standards
- [RUL-002](./RUL-002.md) — Apply standards to a repository area
- [RUL-003](./RUL-003.md) — Inspect the Rules governing current work

### Context

- [CTX-001](./CTX-001.md) — Describe how a repository area fits into the system
- [CTX-002](./CTX-002.md) — Require build proof for changes to an area
- [CTX-003](./CTX-003.md) — Understand the policy affected by a change

### Work

- [WRK-001](./WRK-001.md) — Start work from a durable request
- [WRK-002](./WRK-002.md) — Perform and track development work
- [WRK-003](./WRK-003.md) — Checkpoint and resume work
- [WRK-004](./WRK-004.md) — Rebase work onto its target branch
- [WRK-005](./WRK-005.md) — Prepare a candidate for integration
- [WRK-006](./WRK-006.md) — Inspect finalized Work history

### Build

- [BLD-001](./BLD-001.md) — Build a repository area for development feedback
- [BLD-002](./BLD-002.md) — Prove that all affected work builds

### Review

- [REV-001](./REV-001.md) — Review work in progress
- [REV-002](./REV-002.md) — Accept an exact candidate through independent review

### Integrate

- [INT-001](./INT-001.md) — Integrate prepared work into the target branch
- [INT-002](./INT-002.md) — Resume an interrupted integration safely
