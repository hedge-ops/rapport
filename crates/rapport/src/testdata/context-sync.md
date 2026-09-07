# rapport context show

## `ROOT`

- `path` — .
- `scope` — inherited
- `purpose` — Repository architecture.
- `embedded Ruleset` — `ROOT`

- `source` — `context.toml`

### Ownership — Prefer Here

none

### Boundaries — Avoid Here

none

### Context Rules

none
## `SYNC`

- `path` — app/core/workspace_sync
- `scope` — direct
- `purpose` — Coordinates encrypted synchronization and transfer state.
- `embedded Ruleset` — `SYNC`

- `source` — `app/core/workspace_sync/context.toml`
- `type` — `crate`

### Ownership — Prefer Here

- `SYNC_OWNERSHIP_001` — Owns synchronization scheduling and transfer coordination.

### Boundaries — Avoid Here

- `SYNC_BOUNDARY_001` — Document invariants and mutations belong to the domain component.

### Context Rules

- `SYNC_001` — Keep synchronization state transitions within this component.

## Effective Shared Rulesets

- `TEAM` — Team standards. — source repository — 1 Rules — digest `636e675f878b86213f9a262e1fe977b97d9c5bef392a08ed6363fcba9086dfa6` — declared by ROOT (inherited, direct composition), SYNC (direct, direct composition)

Use `rapport ruleset show <RULESET_ID>` for complete shared Rules.

- `policy digest` — `c1be7ec39bc7fd09178d715d765139e486c797d06952ebc0992ab3c78a337edf`
