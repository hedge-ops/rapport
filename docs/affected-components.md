# Discover affected components

Available in Rapport **0.9.0** (with `rapport-git` **0.2.0**). This is a local,
read-only discovery command. It never fetches refs, changes checkouts, executes a
review, or creates lifecycle records. Supply locally available refs yourself.

## Scopes

```sh
# Staged, unstaged, and untracked files; excludes ignored untracked files.
rapport context affected --pending

# Committed branch changes since the merge base with origin/main.
rapport context affected --base origin/main

# Full local review, including committed and pending changes.
rapport context affected --base origin/main --pending

# Automated PR callers supply the exact comparison revisions.
rapport context affected --base "$BASE_SHA" --head "$HEAD_SHA" --json
```

An explicit scope is required. `--head` requires `--base` and defaults to `HEAD`;
Rapport never infers a source branch. Branch comparison resolves both revisions,
finds their merge base, and compares that ancestor through the selected head.
Changes unique to the base branch are excluded. Pending scope excludes already
committed changes. Combined scope requires the selected head to resolve to the
checkout's `HEAD`, even when supplied through an alias or SHA.

The checkout must have a commit. Resolve merge conflicts before using pending
scope. Invalid refs, missing merge bases, a missing Git repository, invalid policy,
unmapped files, and removed components fail with exit status 2, diagnostics on
stderr, and no partial result on stdout. An empty changeset succeeds with an
empty component array (or no text output).

## Ownership and policy

Every changed path belongs to its nearest enclosing `context.toml`. Ordinary
source edits select only their owners; parent selection does not imply child
selection. Additions, deletions, and both sides of renames participate. Historical
ownership is read from the earlier tree, so a deleted file retains its owner.

Branch discovery resolves policy from the merge-base and selected-head trees;
unrelated working-tree edits cannot affect it. Pending discovery compares HEAD
to the index and the index to the working tree, including untracked files. This
preserves staged evidence even when an unstaged edit reverses it. Combined scope
unions both selections. Context files and installed shared packs must be valid
in every evaluated snapshot, including the index. Stage related context and pack
changes together if an intermediate index otherwise has unresolved includes.

Inherited architecture and rules, including transitive shared standards, are
resolved by the same policy machinery as `rapport review`. When effective review
guidance changes, affected consumers are selected with the responsible policy
sources. Formatting-only policy edits select their file owner without expanding
to consumers. Changes to explicit `components` membership do not recursively
select members; that field retains its documented architecture-only meaning.

A selected component whose declaration is absent from the final selected tree
(or worktree for pending scope) causes an explicit error. Rapport cannot produce
a current review prompt for that removed component. Review the deletion against
the earlier tree explicitly; substituting the parent would lose its guidance.

For an explicit head other than the checkout, returned paths describe that head.
Generate review prompts in a checkout of that revision. Discovery itself does
not switch revisions. Run it without concurrent index or worktree edits; local
Git and filesystem reads are not an atomic snapshot.

## Output contract (schema version 1)

Default output is sorted, unique repository-relative paths, one per line, using
`.` for the root. Paths containing spaces or shell metacharacters use POSIX
single-quote escaping, for example:

```text
.
app
'other space'
```

For automation, use JSON and pass each `components[].path` as a separate argument
to `rapport review --`. Do not split paths on whitespace or evaluate output as
shell code. JSON paths are literal repository-relative strings, without quoting.
All arrays of paths and the component array are sorted lexicographically and
deduplicated. Fields remain present even when null or empty:

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer `1`; consumers should reject unsupported versions. |
| `comparison` | Null for pending-only; otherwise the object below. |
| `comparison.base` | Resolved base commit ID. |
| `comparison.head` | Resolved selected head commit ID. |
| `comparison.merge_base` | Resolved merge-base commit ID used for the diff. |
| `pending.included` | Whether pending scope was requested. |
| `pending.head` | Checkout commit ID for pending scope; otherwise null. |
| `components[].path` | Component directory relative to the repository root. |
| `components[].changed_files` | Changed paths actually owned by this component in a compared snapshot. Includes old and new rename paths under their respective owners. |
| `components[].policy_sources` | Context or shared-pack paths whose changed effective policy caused selection. These need not be files owned by the selected component. |

Commit IDs are full Git object IDs (SHA-1 or SHA-256 depending on the repository).
Consumers should tolerate additive object fields within schema version 1.

Complete pending-only example, after an ancestor policy edit:

```json
{
  "schema_version": 1,
  "comparison": null,
  "pending": {
    "included": true,
    "head": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  },
  "components": [
    {
      "path": ".",
      "changed_files": ["context.toml"],
      "policy_sources": ["context.toml"]
    },
    {
      "path": "app",
      "changed_files": [],
      "policy_sources": ["context.toml"]
    }
  ]
}
```

Complete committed-only example (illustrative commit IDs):

```json
{
  "schema_version": 1,
  "comparison": {
    "base": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "head": "cccccccccccccccccccccccccccccccccccccccc",
    "merge_base": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  },
  "pending": { "included": false, "head": null },
  "components": [
    {
      "path": "app",
      "changed_files": ["app/src/lib.rs"],
      "policy_sources": []
    }
  ]
}
```

Complete combined example for committed and pending changes in the same component:

```json
{
  "schema_version": 1,
  "comparison": {
    "base": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "head": "cccccccccccccccccccccccccccccccccccccccc",
    "merge_base": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  },
  "pending": {
    "included": true,
    "head": "cccccccccccccccccccccccccccccccccccccccc"
  },
  "components": [
    {
      "path": "app",
      "changed_files": ["app/README.md", "app/src/lib.rs"],
      "policy_sources": []
    }
  ]
}
```

A successful empty pending selection uses `comparison: null`, the same pending
metadata, and `components: []`.

## Downstream integration

This Python example passes paths without shell parsing and avoids calling review
with no paths (which would otherwise review the root):

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
if selection["schema_version"] != 1:
    raise RuntimeError("Unsupported Rapport affected schema")
paths = [component["path"] for component in selection["components"]]
if paths:
    subprocess.run(["rapport", "review", "--", *paths], check=True)
```

Run prompt generation at the selected head. Keep the changed-file and policy-source
evidence alongside the review input. `prs` owns PR comparison selection and
orchestration; Rapport owns component selection and prompt generation.

## Release and People Work adoption

This change prepares `rapport` 0.9.0 and `rapport-git` 0.2.0; these versions are
not released merely by editing the manifests. After review, commit and merge the
change, then dispatch the repository's **Publish** workflow on that commit:

```sh
gh workflow run publish.yml --ref main -f crate=rapport-git
# Wait for rapport-git 0.2.0 publication to succeed before publishing Rapport.
gh workflow run publish.yml --ref main -f crate=rapport
```

The workflow runs `just ci`, publishes to crates.io, and creates tags
`rapport-git-v0.2.0` and `rapport-v0.9.0`. Rapport's release also builds the
platform archives consumed by `cargo binstall`. Verify both publication and the
archive jobs before telling downstream consumers the release is ready.

People Work can then require Rapport 0.9.0 or later, update provisioning/version
pins, replace its internal file-to-component mapping with the explicit-SHA JSON
command, and pass every returned path to review. Keep its local skill and PR
integration changes in the existing People Work development-process PR.
