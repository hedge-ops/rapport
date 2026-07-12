# Rapport Specification Instructions

This directory contains the canonical product requirements for Rapport.

## Requirement Shape

- Give every requirement a stable category ID.
- Keep each requirement focused on one meaningful developer journey.
- Put supporting mechanics, invariants, and refusal boundaries in that
  journey's scenarios instead of creating subsystem checklist requirements.
- State why the journey matters in `## Intent`.
- Write observable behavior under `## Scenarios` using italicized `_Given_`,
  `_When_`, `_Then_`, `_And_`, and `_But_` keywords.
- Record unresolved product decisions in `GAPS.md`, not as settled behavior.
- Keep implementation status, design plans, and issue discussion outside
  requirement bodies.

## Frontmatter

Each requirement uses TOML frontmatter with:

- `category`: the requirement ID prefix.
- `domain`: the narrower product concept.
- `capability`: the developer-facing outcome.
- `status`: `draft`, `supported`, `partial`, or `retired`.
