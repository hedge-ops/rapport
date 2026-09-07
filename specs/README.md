# Rapport Specifications

Rapport defines repository architecture and review benchmarks, resolves their
inheritance, and prepares sourced prompts for humans and agents. Repository tools
own development, testing, builds, and integration.

## Active requirements

- [RUL-001](RUL-001.md) — Create and reuse shared repository standards
- [RUL-002](RUL-002.md) — Apply standards to a repository area
- [RUL-003](RUL-003.md) — Inspect standards governing selected components
- [CTX-001](CTX-001.md) — Describe how a repository area fits into the system
- [CTX-004](CTX-004.md) — Validate architecture and benchmark declarations
- [REV-003](REV-003.md) — Prepare a component review without lifecycle state

Requirements use stable category IDs, an intent, and observable scenarios.
Unresolved decisions belong in [GAPS.md](GAPS.md).

## Retired lifecycle requirements

CTX-002, CTX-003, WRK-001 through WRK-006, BLD-001 and BLD-002, REV-001 and
REV-002, and INT-001 and INT-002 are retained as historical records with status
`retired`. Their Work, signoff, acceptance, and integration behavior is outside
Rapport's scope. See the [migration guide](../docs/lifecycle-migration.md).
