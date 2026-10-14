# rapport ruleset show --rule

- `id` — CHEF_TESTING_001
- `owning ruleset` — CHEF_TESTING
- `reference` — none

Declare directory permissions explicitly.

## Rationale

Explicit permissions keep Chef directory resources reproducible.

## Avoid

```ruby
directory '/var/lib/people-work' do
  owner 'root'
end

```

## Prefer

```ruby
directory '/var/lib/people-work' do
  owner 'root'
  mode '0700'
end

```
