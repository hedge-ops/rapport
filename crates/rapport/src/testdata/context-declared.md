# rapport context show

## `APP`

- `path` — app
- `scope` — direct
- `purpose` — Updated application policy.
- `embedded Ruleset` — `APP_RULE`

- `source` — `app/context.toml`

### Ownership — Prefer Here

- `APP_OWNERSHIP_002` — User-facing application behavior.

### Boundaries — Avoid Here

- `APP_BOUNDARY_001` — Repository automation belongs at root. — owner `ROOT`

### Context Rules

- `APP_RULE_001` — Keep UI policy in the app.

## Effective Shared Rulesets

- `TEAM` — Team policy. — source repository — 0 Rules — digest `6ea62ff21aa35350fad41f707131994c0e62bf130b1c568765d59e793074b7cd` — declared by APP (direct, direct composition)

Use `rapport ruleset show <RULESET_ID>` for complete shared Rules.

- `policy digest` — `cf3ebb0e9959e24554172b20c84f6035b0115b8b149d1c384de0406c2722eb79`
