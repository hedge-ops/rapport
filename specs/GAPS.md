# Rapport Specification Gaps

These decisions remain open and are not settled requirements.

## 1. Task Model

Decide whether Work tasks remain a lightweight corrective-action ledger or
become a general dependency-aware task system.

Affected requirements: WRK-002, BLD-001, REV-001, REV-002.

## 2. Commit Adoption

Decide whether Rapport always creates checkpoints or can adopt commits created
directly with Git.

Affected requirement: WRK-003.

## 3. Dirty Snapshot Identity

Define which dirty working-tree states can receive reproducible ad hoc build or
review identities.

Affected requirements: BLD-001, REV-001.

## 4. GitHub Evidence

Define how local build proof maps to GitHub Actions and status checks without
making GitHub the authoritative store for the Work log.

Affected requirements: CTX-002, BLD-002, INT-001.

## 5. Integration Policy

Define supported merge methods, branch deletion, protected branches, and
recovery boundaries.

Affected requirements: INT-001, INT-002.

## Roadmap References

- Plan phase: GitHub issue #104.
- Ship phase: GitHub issue #105.
- Context-aware review planning: GitHub issue #101.
- Explicit build contracts: GitHub issue #103.
- Integration orchestration and recovery: GitHub issue #99.
