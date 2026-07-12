# Rapport Specification Gaps

There are no unresolved decisions in the current Rules, Context, Work, Build,
Review, and Integrate baseline. Add new gaps here when a requirement exposes a
product decision that has not yet been made.

## Roadmap References

- Plan phase: GitHub issue #104.
- Ship phase: GitHub issue #105.
- Context-aware review planning: GitHub issue #101.
- Explicit build contracts: GitHub issue #103.
- Integration orchestration and recovery: GitHub issue #99.

## Future Review Scaling

The baseline has one Review Task, one acceptance outcome, and one Review Unit
containing the complete candidate and every applicable Rule. A future delivery
may partition detailed Rules across several independent Review Units while
giving every unit the complete intent and candidate overview.

The exact partitioning and synthesis behavior remains intentionally unsettled.
A limit of approximately 50 detailed Rules per unit is the current working
direction, not a baseline requirement.
