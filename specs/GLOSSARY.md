# Rapport Glossary

## Policy

The effective Context and Rules for an affected path. Policy states what a
repository area means and what must be true of changes to it.

## Candidate

An exact proposed change identified by its target/source merge base, source
commit, content checksum, and affected files. Its committed files are the
source-side diff from that merge base through source `HEAD`, excluding commits
and files present only on the target. A development snapshot additionally
includes staged, unstaged, deleted, and relevant untracked files. A committed
candidate can receive acceptance proofs; a dirty snapshot can receive feedback
but cannot become the final accepted candidate.

## Proof

Durable evidence that an operation or independent review evaluated an exact
candidate under an exact policy. Proof prevents accidental reuse of evidence
for mismatched inputs under a trusted local operator and agent host; it is not a
tamper-resistant security attestation.

## Signoff

The determination that a required proof is current and passing for the exact
candidate being evaluated.

## Work Log

The durable local ledger of the request, repository identities, Tasks, results,
proofs, decisions, and current state of one body of work.

## Work History

The local, cross-repository collection of finalized Work logs. Each immutable,
schema-versioned record preserves human prose, dates, Git identities, Tasks,
proof, and decisions as transparent TOML outside every repository. History is
not telemetry and is never uploaded implicitly.

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

A workflow-owned requested or performed unit of Work, including its origin,
status, Git state, timing, result, and output. Failed and superseded Tasks remain
immutable history. Corrective work and retries are new causally related Tasks.
Tasks may be ordered to keep the next action clear, but they do not form a
dependency graph.

## Checkpoint

An ordinary Git commit that preserves a coherent development state and is
recorded in the Work log. Git remains the source of truth whether Rapport or the
developer created the commit.

## Build

Execution of an explicit repository-owned operation through Rapport. A build
can provide development feedback or proof for an exact committed candidate.

## Build Stage

A nonnegative order assigned to a required Build operation. Stages execute in
ascending order, while operations in the same stage may run concurrently.

## Machine Resource Group

An optional name that limits a Build operation to one concurrent execution
across Rapport processes and worktrees on the same machine. It does not
coordinate work across different machines.

## Dirty State

A repository state with staged, unstaged, or relevant untracked changes. Work
can retain Tasks completed in dirty state as history and feedback, but those
Tasks cannot sign off the underlying Git commit.

## Review

Judgment of a complete source-control change using every affected Context and
Rule. An acceptance Review is independent, happens before publication, and
binds to the exact candidate it evaluated. One Review Task owns one acceptance
outcome and contains one or more Review Units; the initial delivery creates one
unit for the complete candidate.

## Review Unit

One independently evaluated packet inside a Review Task. Every unit receives
the complete intent and candidate overview. The initial delivery uses exactly
one unit containing every applicable Rule; future delivery may partition the
detailed Rules without splitting the acceptance outcome.

## Integrate

The phase that publishes an accepted candidate, creates a pull request carrying
the candidate and its Review evidence as the aggregate shared Review artifact,
verifies the aggregate `Rapport Build` result against its latest head commit,
observes every reported pull-request check, and moves the candidate onto the
target branch when those checks are terminal and non-failing. Rapport owns its
acceptance policy and does not publish a duplicate Review status for the pull
request.
