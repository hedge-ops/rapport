# Rapport Glossary

## Policy

The effective Context and Rules for an affected path. Policy states what a
repository area means and what must be true of changes to it.

## Candidate

An exact proposed change identified by its base commit, source commit, content
checksum, and affected files. A committed candidate can receive acceptance
proofs. A dirty working-tree snapshot can receive feedback but is not a final
accepted candidate.

## Proof

Durable evidence that an operation or independent review evaluated an exact
candidate under an exact policy.

## Signoff

The determination that a required proof is current and passing for the exact
candidate being evaluated.

## Work Log

The durable local ledger of the request, repository identities, tasks, attempts,
results, proofs, decisions, and current state of one body of work.

## Context

The repository-owned description of a folder's purpose, ownership, boundaries,
applicable Rules, and required build signoffs. Descendant paths accumulate
applicable ancestor Context.

## Purpose

The responsibility of a repository area and the reason it exists within the
larger system.

## Ownership

The responsibilities and artifacts a repository area is expected to control.

## Boundaries

Statements describing what a repository area must not own, cross, expose, or
silently absorb.

## Rule

An individually identified standard stating what must be true of work governed
by the Rule.

## Ruleset

A built-in, repository, or Context-owned collection of Rules that can include
other rulesets by stable ID.

## Task

A durable actionable obligation in Work, including its origin, status,
attempts, and result. Tasks may be ordered to keep the next action clear, but
they do not form a dependency graph.

## Attempt

One execution of a task, build, or review with a unique ID. Its evidence records
the relevant Git commit and whether repository changes were present.

## Checkpoint

An ordinary Git commit that preserves a coherent development state and is
recorded in the Work log. Git remains the source of truth whether Rapport or the
developer created the commit.

## Build

Execution of an explicit repository-owned operation through Rapport. A build
can provide development feedback or proof for an exact committed candidate.

## Dirty State

A repository state with staged, unstaged, or relevant untracked changes. Work
can retain attempts made in dirty state as history and feedback, but those
attempts cannot sign off the underlying Git commit.

## Review

Judgment of a complete source-control change using every affected Context and
Rule. An acceptance review is independent and binds to the exact candidate it
evaluated.

## Integrate

The phase that creates a pull request for prepared Work, obtains and verifies
the required GitHub signoffs against its latest head commit, and moves it onto
the target branch.
