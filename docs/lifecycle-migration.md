# Review-focused command scope

Rapport's supported entry points are `review`, `context`, `ruleset`, `init`, and
`prime`. Architecture and benchmarks are repository-owned inputs. Review emits a
Markdown prompt for an external human or agent, without owning development state.

## Compatibility in this release

- `rapport review [PATH ...]` generates a sourced prompt directly. To review a
  directory named `start`, `status`, or another legacy review verb, use `./start`
  or `-- start` to disambiguate.
- `work`, `develop`, `build`, `integrate`, `github`, and the integration `doctor`
  remain callable but are deprecated and hidden from root help.
- `review start`, `complete`, `reconcile`, `override`, `cancel`, and `status` retain
  their old lifecycle behavior and are hidden from review help.
- Legacy context grades and signoffs remain readable and editable. They do not
  gate direct prompt generation. `context doctor` still validates legacy workflow
  declarations; it is not a prerequisite for review.
- `init` creates review guidance and ignore entries, without creating the shared
  GitHub signoff workflow. Explicit legacy signoff creation still writes it.
- Existing Work history, local state, and GitHub configuration are left intact.

## Proposed removal in the next breaking release

Remove Work and Task management, development checkpoints, build orchestration,
integration and GitHub setup, and the lifecycle review result/acceptance model.
Remove signoff workflow generation and grade-based acceptance from context
commands. Keep architecture CRUD, reusable standards, inheritance validation,
and sourced review prompts. Refocus `context doctor` solely on context validity.

Repositories should move build/test commands, commits, pull requests, and merge
policy into their own tools before adopting that release. Finish or archive active
legacy Work while these commands are available. Owners should inspect generated
Rapport workflows and remote settings before removing anything; this release
performs no automatic cleanup or deletion.

Removal is a proposal, not a silent behavior change in this release. The
[legacy reference](legacy-workflow.md) describes the retained commands.
