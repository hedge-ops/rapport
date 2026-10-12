# Discover and review affected components

Rapport **0.10.0** returns schema **2**, including revision-tagged review prompts
for surviving and removed components. Discovery is local and read-only: it never
fetches refs, changes checkouts, executes reviews, or creates lifecycle records.
Supply locally available refs yourself.

## Scopes

```sh
# Staged, unstaged, and non-ignored untracked changes.
rapport context affected --pending

# Committed changes since the merge base with origin/main.
rapport context affected --base origin/main

# Committed and pending changes together.
rapport context affected --base origin/main --pending

# Exact revisions for automated PR review, regardless of the checkout.
rapport context affected --base "$BASE_SHA" --head "$HEAD_SHA" --json
```

An explicit scope is required. `--head` requires `--base` and defaults to `HEAD`.
Rapport resolves both revisions and compares their merge base through the selected
head. Changes unique to the base branch are excluded. Combined scope requires the
selected head to equal the checkout's `HEAD`. The checkout must have a commit.

Invalid refs, unavailable history or merge bases, missing repositories, invalid
policy, unmapped files, and pending merge conflicts fail with exit status 2,
diagnostics on stderr, and no partial stdout. An empty changeset succeeds.

## Ownership and policy

Every changed path belongs to its nearest enclosing `context.toml` in each
relevant snapshot. Additions, deletions, and both sides of renames participate.
Parent selection does not imply child selection. A moved declaration selects both
its old and new component paths. Removing only a declaration still selects its
historical component; any surviving changed files also select their new owner.

Committed comparisons load policy from the merge-base and selected-head trees.
Unrelated working-tree edits cannot affect discovery or prompt generation.
Pending discovery compares HEAD to the index and the index to the worktree,
including untracked files. Staged evidence survives unstaged reversals. Combined
scope unions these selections. Context and installed shared packs must be valid
in every evaluated snapshot, including the index. Stage related changes together
if an intermediate index would otherwise have unresolved includes.

Inherited architecture, rules, and transitive shared standards use the same
policy machinery and prompt builder as `rapport review`. Effective guidance
changes select affected consumers with responsible policy sources. Formatting
changes select their file owner without expanding to consumers. Explicit
`components` membership remains architecture-only.

Each selected component has exactly one entry in `reviews`, matched by `path`:

- A surviving declaration uses the selected-head policy for committed-only scope,
  or final worktree policy when pending changes are included.
- A removed declaration uses the last compared snapshot in which it existed:
  merge base for committed deletion, HEAD for staged deletion, or index for
  unstaged deletion. Its prompt includes its earlier ancestors and shared packs,
  even when those policies are changed or removed in the final tree.
- Pending intermediate components can have an index policy snapshot even when
  they exist in neither HEAD nor the final worktree. Index and worktree sources
  are labeled explicitly; their `head` is a baseline commit, not a claim that
  their policy was committed there.

These prompts describe policy and scope; callers must still supply the diff and
inspect relevant code. Retain comparison, pending, changed-file, policy-source,
and policy-snapshot metadata with the review input. Concurrent index/worktree
edits are unsupported: local reads are not an atomic snapshot.

## JSON contract (schema version 2)

JSON paths are literal repository-relative strings, with `.` for the root.
Component, review, and path arrays are sorted lexicographically and deduplicated.
Fields remain present when null or empty. Reject unsupported schema versions;
tolerate additive object fields within version 2.

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer `2`. |
| `comparison` | Null for pending-only; otherwise comparison revisions. |
| `comparison.base` | Resolved requested base commit. |
| `comparison.head` | Resolved selected head commit. |
| `comparison.merge_base` | Actual earlier commit used for the diff. |
| `pending.included` | Whether pending scope was requested. |
| `pending.head` | Checkout commit for pending scope; otherwise null. |
| `components[].path` | Selected component directory. |
| `components[].changed_files` | Changed paths owned in a compared snapshot, including both sides of moves. |
| `components[].policy_sources` | Changed context/shared-pack sources causing effective policy selection. |
| `reviews[].path` | Component directory; exactly matches a selected component. |
| `reviews[].removed` | Whether the declaration is absent from the final selected tree. |
| `reviews[].policy_snapshot.kind` | `commit`, `index`, or `worktree`. |
| `reviews[].policy_snapshot.revision` | Full policy commit ID; present only for `commit`. |
| `reviews[].policy_snapshot.head` | Full baseline commit ID; present only for `index` and `worktree`. |
| `reviews[].prompt` | Complete Markdown review prompt with scope, policy provenance, architecture, and standards. |

Git object IDs are full SHA-1 or SHA-256 IDs as applicable to the repository.
`reviews` is empty exactly when `components` is empty.

Default text output remains shell-quoted component paths, one per line. It fails
explicitly if any component was removed, directing callers to `--json`; bare
paths cannot preserve historical prompt provenance. Do not split paths on
whitespace or evaluate text output as shell code.

## Automated consumers and migration from schema 1

Schema 1 consumers generated current prompts with `rapport review <paths>` and
failed discovery for deletions. Schema 2 consumers should use the embedded
prompts for **all** components, so mixed scope and explicit heads require no
checkout restoration or duplicate ownership mapper:

```python
import json
import os
import subprocess

result = subprocess.run(
    ["rapport", "context", "affected", "--base", os.environ["BASE_SHA"],
     "--head", os.environ["HEAD_SHA"], "--json"],
    check=True, capture_output=True, text=True,
)
selection = json.loads(result.stdout)
if selection["schema_version"] != 2:
    raise RuntimeError("Unsupported Rapport affected schema")
components = {component["path"]: component for component in selection["components"]}
assert set(components) == {review["path"] for review in selection["reviews"]}
for review in selection["reviews"]:
    component = components[review["path"]]
    # Supply review["prompt"], component evidence, selection provenance,
    # and the actual diff/code to the automated reviewer.
    print(review["prompt"])
```

An empty selection performs no review. Never regenerate a removed component's
prompt from its parent or selected head. `prs` owns PR orchestration; Rapport owns
complete component selection and snapshot-specific prompt generation.

## Release and People Work adoption

The manifest prepares Rapport 0.10.0; it does not publish it. After review and
merge, dispatch the repository's Publish workflow on the merged commit:

```sh
gh workflow run publish.yml --ref main -f crate=rapport
```

The workflow runs `just ci`, publishes to crates.io, and creates the
`rapport-v0.10.0` tag and platform archives. Verify publication and archive jobs
before updating downstream version pins. `rapport-git` 0.2.0 is unchanged.

People Work's automated PR consumer must accept schema 2 and use `reviews[].prompt`
instead of invoking current-tree review for each path. Verify mixed surviving and
removed scope using PR #1947's explicit comparison revisions after provisioning
the new release. No consumer should silently drop removed entries.
