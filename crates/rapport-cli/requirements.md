# rapport-cli — Requirements

## Audience

rapport-cli is written for agents writing CLIs that agents consume.
Humans are there too — they observe, they review, they sometimes
type — but the load-bearing audience is the agent on both sides of
every command. Every requirement below exists to serve that
audience in that order.

## R1. Structural — every CLI has

- R1.1  A **default summary**. Invoking the CLI with no arguments
        shows current state — in-progress work, due items, next
        actions. `summary` is not a subcommand; the bare CLI is
        the summary.

- R1.2  A `prime [topic]` command. Returns embedded reference
        documentation. Always accepts an optional topic; with no
        topic, returns the default topic (which may itself be an
        index of subtopics). No external dependencies.

- R1.3  A `doctor` command. Returns health checks with outcomes
        (ok / warn / fail). Takes no domain arguments, but accepts
        environment-variable overrides for the things it would
        otherwise read from the environment. Runs every check to
        completion; exits non-zero if any check is `fail`.

- R1.4  A single executable name per CLI. Each rapport CLI is its
        own binary; rapport CLIs are not subcommands of a meta-CLI.

## R2. Command shape

Rapport CLIs follow a crux-shaped pattern: the domain declares its
verbs and arguments as enums and types; the framework provides
traits, constraints, parsing, and a starter library.

- R2.1  The CLI's command surface is a domain-declared **verb enum**.
        Each variant is one verb; each variant is tagged with a
        **kind**. The framework requires `Prime` and `Doctor`
        variants in every verb enum — a CLI missing either fails
        to compile.

- R2.2  Every verb has one of two kinds, each with a fixed contract:
          - **Lookup** — read-only; safe to re-run; output ends
            with next-actions.
          - **Action** — mutates state; output reports what changed,
            then ends with next-actions.
        State machines, preconditions, and inverses are domain
        logic; the framework does not peer into them.

- R2.3  First positional argument is the verb name. A second
        positional argument is the **noun** (entity type), required
        when the CLI defines more than one entity, absent otherwise.
        Disambiguation is structural, never inferred from ID format.

- R2.4  Verbs requiring an ID error when invoked without one. They
        do not fall back to listing or summarizing. `show` and
        `list` are complementary, not interchangeable.

- R2.5  All flag arguments are **typed**. Domains declare arg types
        by implementing the framework's `Argument` trait. The
        framework provides parsing, help generation, and error-hint
        routing. Ad-hoc string flags are not allowed; every flag
        reduces to a typed argument.

- R2.6  The framework ships a **rapport-cli-common** library of
        starter verbs (e.g. show, list, add, update, delete) and
        starter arg types (Period, PointInTime, Duration, Recurrence,
        Filter, Id<T>, SingleLine, Markdown, Sentiment, Owner).
        Importing from common is conventional but not required.

## R3. Output shape

- R3.1  Output is composed from a closed vocabulary of **prose
        primitives**, provided by `rapport-prose`. Authors compose
        views using prose; they do not free-render. Cross-CLI
        consistency comes from primitive uniformity, not from
        layout uniformity.

- R3.2  Domain logic returns data. **View logic** — written by the
        CLI author — turns that data into prose. The framework
        renders prose to terminal text.

- R3.3  Prose primitives include headings, lists (numbered and
        bulleted), tables, badges (ok/warn/fail), fields, sections,
        run-hints, and next-actions. The vocabulary is closed;
        domains do not invent new primitives.

- R3.4  Every view includes a **next-actions** node. The framework
        enforces presence at compile time — a view without
        next-actions does not compile. Authors write next-actions
        explicitly; the framework does not auto-generate them.

- R3.5  No command syntax appears outside the run-hint primitive.
        The framework rejects command-shaped strings in heading,
        body, label, and field text.

- R3.6  Output is plain text — English and markdown-ish prose.
        Rapport CLIs explain situations to agents and humans in
        natural language; structured machine-readable output is
        out of scope.

- R3.7  The framework ships **view templates** in `rapport-cli-common`
        (e.g. `summary_view`, `show_view`, `list_view`,
        `result_view`) for common shapes. Templates are conventional
        starters; authors may use them, modify them, or compose
        from primitives directly.

## R4. Error shape

- R4.1  Errors are **views with a failure marker**. They use the
        same prose primitives and the same next-actions requirement
        as success views. Errors are not a separate channel.

- R4.2  Every error view is **self-sufficient**: an agent reading
        the error has enough information to act in the next turn
        without making intermediate calls. A well-formed error
        contains:
          - what was attempted (captured by the framework, see R4.3),
          - what went wrong, in context,
          - concrete next-actions to recover.

- R4.3  The framework auto-captures the attempted invocation and
        prepends it to every error view. Authors write diagnosis
        and recovery; the framework guarantees the context line.
        Example:

          You ran: builder app/v2/crates
          That directory does not exist.
          └ run ls app
          └ run find . -name Cargo.toml

- R4.4  Next-actions in an error view are contextual to the error.
        They may be corrections to retry the original command,
        diagnostic commands to inspect state, or prime references
        when the error is a misuse pattern. The framework does
        not constrain the kind of next-action; it only requires
        their presence.

- R4.5  Error views render to stderr; the process exits non-zero.

## R5. Behavior

- R5.1  Commands never prompt for input. Every invocation runs to
        completion and produces output without interactive input.

- R5.2  Actions report what they did in enough detail that an agent
        can detect duplicate application by reading the result view.
        Strict idempotence is not required — some mutations create
        new identifiers each time — but the result must make
        duplication legible.

- R5.3  `prime` runs offline. `doctor` may talk to dependencies, but
        never crashes when they are missing; it reports them as
        failed checks. (See R1.2, R1.3.)

- R5.4  Exit code is 0 on success, non-zero on error. Success views
        go to stdout; error views go to stderr; logs and progress
        messages go to stderr — never mixed with success output on
        stdout.

- R5.5  Action verbs **capture** underlying-tool output rather than
        streaming it. During execution, the CLI is silent on both
        channels; agents reading captured output later don't pay
        for streamed log volume they didn't need. On success, the
        result view contains only the structured summary (status,
        duration, counts). On failure, the error view surfaces a
        relevant slice of captured output — the actual error block,
        not the full log.

## R6. Discoverability

- R6.1  rapport-cli ships a **framework prime** — embedded reference
        documentation from which an agent can write a complete
        rapport CLI without consulting other sources. The framework
        prime follows the same shape as a CLI prime (R1.2):
        invokable as a default topic, optionally topic-indexed.
        If a feature is not in the framework prime, it does not exist.

- R6.2  Each rapport CLI's own `prime` is the single source of
        truth for that CLI's contract. An agent uses
        `<cli> prime [topic]` to learn the CLI; no separate README,
        man page, or external doc is required for agent
        comprehension.

- R6.3  All user-facing text — `<cli> --help`, `<cli> <verb> --help`,
        and error-view hints — is excerpted from the CLI's prime.
        Help, errors, and prime cannot drift; there is one source.

- R6.4  Violations of R1–R5 are enforced at **compile time** wherever
        possible — missing prime or doctor verbs, missing next-actions
        on a view, ad-hoc flag types, command syntax in prose.
        Compile-time enforcement is preferred over runtime linting.

## Resolved questions

These were open during drafting; recording how each closed.

- **Q1.** Does the action/lookup split need a third "reference"
  bucket for prime and `--help`?
  → No. Two kinds only (R2.2). Prime and doctor are lookups; their
  next-actions requirement (R3.4) still applies.

- **Q2.** When a CLI has zero entities (e.g. a builder), is the noun
  slot forbidden, or does the verb act on an implicit subject?
  → Forbidden. Noun is absent when the CLI defines zero or one
  entities (R2.3). Action-style CLIs have only verbs and typed args.

- **Q3.** Are workflow verbs (start/stop/done/assign/…)
  framework-fixed or domain-declared?
  → Domain-declared via the verb enum (R2.1). The framework ships
  starters in rapport-cli-common (R2.6); domains import or declare
  their own. Transitions are not a distinct kind — they are
  actions (R2.2).

- **Q4.** Is `summary` a lookup with a fixed layout, or is it the
  one place a CLI gets editorial freedom?
  → Neither. The bare CLI is the summary (R1.1); the view is
  composed from prose primitives like any other view. Uniformity
  comes from the prose vocabulary, not from a fixed layout.

- **Q5.** Multi-tenancy / context: is workspace selection a
  framework concern or a domain concern?
  → Domain concern. The framework is silent on context. Doctor
  reports missing context as a failed check (R1.3); errors from
  missing context use the standard error pattern (R4).

## Glossary

Terms that gained specific meaning during drafting.

- **Verb** — A variant in a CLI's command enum, tagged with a
  kind. Domain-declared.
- **Kind** — Lookup or action. Determines whether a verb mutates
  state and shapes its output contract.
- **Noun** — Entity type. Second positional argument, present only
  when the CLI defines more than one entity.
- **View** — An author-composed prose tree representing one
  command's output. Always includes a next-actions node.
- **Prose** — The closed vocabulary of output primitives provided
  by `rapport-prose`. The unit of cross-CLI consistency.
- **Next-actions** — Required prose node listing suggested next
  commands with run-hints. Present on every view, including errors.
- **Run-hint** — Structured representation of a command, rendered
  as `└ run …`. The only place command syntax appears in output.
- **Prime** — Embedded reference documentation; the single source
  of truth for a CLI's contract. Topic-indexed, default-topic on
  bare invocation.
- **Doctor** — Health-check command. Reports dependency status
  without crashing.
- **Argument** — Framework trait that domain types implement to
  act as flag types.
- **rapport-cli-common** — Companion library of starter verbs,
  arg types, and view templates. Conventional, not required.
