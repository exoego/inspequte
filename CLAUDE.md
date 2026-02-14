# CLAUDE.md

## Project context

- Repo: inspequte (CLI command: `inspequte`).
- Purpose: fast, CLI-first static analysis for JVM class/JAR files.
- Output: SARIF v2.1.0 only.
- License: AGPL-3.0.
- Commit style: Conventional Commits v1.0.0.

## Build & test commands

```bash
cargo fmt                        # format after every code change
cargo build                      # compile
cargo test                       # run all tests (needs JAVA_HOME pointing to Java 21)
cargo audit --format sarif       # security audit (install: cargo install cargo-audit --locked)
```

## Key decisions

- Add documentation comments to each struct.
- Use Java 21 for the test harness via `JAVA_HOME`.
- Run `cargo fmt` after each code modification.
- Span naming convention: `scope.action` (e.g., `scan.jar`, `scan.class`).
- Rule messages must be intuitive and actionable: clearly state the problem and what to do.
- Do NOT support suppression annotations (`@Suppress` / `@SuppressWarnings`).
- Support only JSpecify annotations for annotation-driven rule semantics.

## Test harness guidelines

- Use meaningless, generic names in test harness Java code (`ClassA`, `ClassB`, `MethodX`, `varOne`).
- Avoid names matching examples in issues/docs to prevent accidental test passing.
- Exception: use meaningful names when testing actual JDK/library APIs.

## Working with plans

- Review `plans/` directory before starting new features.
- On completion: rename plan file with `.done.md` suffix (e.g., `01-foo.md` -> `01.foo.done.md`).
- Add 2-3 bullet post-mortem notes to completed plan files.

## Release checklist

- Run `cargo test`, `cargo build`, and `cargo audit --format sarif`.
- Add/maintain SARIF sanity checks via tests.
- Update `README.md`/`CONTRIBUTING.md` if CLI behavior or rule coverage changed.
- Run `scripts/generate-rule-docs.sh` after rule changes.

---

## Rule authoring workflow

Use `@prompts/authoring-rule.md` as the orchestration prompt for end-to-end rule development. The workflow uses isolated subagents per phase to reduce context mixing.

### Phase overview

| Phase | Prompt | Skill reference | Primary output |
|-------|--------|-----------------|----------------|
| 1. Ideation | `prompts/ideate-rule.md` | - | `rule-id` + `rule idea` |
| 2. Plan | `prompts/authoring-plan.md` | `.codex/skills/inspequte-rule-plan/SKILL.md` | `src/rules/<rule-id>/plan.md` |
| 3. Spec | `prompts/authoring-spec.md` | `.codex/skills/inspequte-rule-spec/SKILL.md` | `src/rules/<rule-id>/spec.md` |
| 4. Impl | `prompts/authoring-impl.md` | `.codex/skills/inspequte-rule-impl/SKILL.md` | Rule code + tests |
| 5. Verify | `prompts/authoring-verify.md` | `.codex/skills/inspequte-rule-verify/SKILL.md` | Go/No-Go report |
| Resume | `prompts/authoring-no-go-resume.md` | `.codex/skills/inspequte-rule-no-go-resume/SKILL.md` | Resumed impl |

### Recommended sequence

1. **Ideation**: Launch a subagent with `prompts/ideate-rule.md`.
   - Reference `prompts/references/no-go-history.md` to avoid duplicate ideas.
   - Output: `rule-id` and `rule idea` (2 lines only).
2. **Plan**: Launch a subagent with `prompts/authoring-plan.md` using the `rule-id` and `rule idea`.
   - Creates `src/rules/<rule-id>/plan.md` with risk checklist.
3. **Spec**: Launch a subagent with `prompts/authoring-spec.md`.
   - Creates `src/rules/<rule-id>/spec.md` (the behavior contract).
4. **Impl-Verify loop** (up to 3 iterations):
   - Launch `impl` subagent with `prompts/authoring-impl.md`.
   - Prepare verify inputs:
     ```bash
     scripts/prepare-verify-input.sh <RULE_ID> [<BASE_REF>]
     cargo build > verify-input/reports/cargo-build.txt 2>&1
     cargo test > verify-input/reports/cargo-test.txt 2>&1
     cargo audit --format sarif > verify-input/reports/cargo-audit.sarif
     ```
   - Launch `verify` subagent with `prompts/authoring-verify.md` using only `verify-input/`.
   - If `Go`: stop looping.
   - If `No-Go`: feed `verify-input/verify-report.md` findings into next `impl` iteration.
5. If still `No-Go` after 3 iterations, stop and surface blockers.
6. Regenerate rule docs: `scripts/generate-rule-docs.sh`.

### Subagent contract

- Launch one subagent per phase.
- Pass only the minimum required inputs to each subagent.
- Carry forward only phase outputs (do not forward full chat logs).
- Treat each phase output as the next phase input contract.

### Non-negotiable rules

- `spec.md` is the contract; do not modify it unless explicitly instructed.
- Verify must use only files under `verify-input/`.
- Verify must not read `plan.md` or implementation discussion logs.

---

## Rule implementation quick reference

When implementing rules (phase 4), follow these key points from `.codex/skills/inspequte-rule-impl/SKILL.md`:

- Read `src/rules/<rule-id>/spec.md` as the contract.
- Add `#[derive(Default)]` to the rule struct and `crate::register_rule!(RuleName);` after it.
- Implement `Rule::run` with `AnalysisContext`.
- Guard class scans with `if !context.is_analysis_target_class(class) { continue; }`.
- Add harness tests using `JvmTestHarness` with Java 21.
- If adding a new rule module, declare it in `src/rules/mod.rs`.
- Update snapshots when registered rule set changes: `INSPEQUTE_UPDATE_SNAPSHOTS=1 cargo test sarif_callgraph_snapshot`.
- Run `cargo fmt`, then `cargo build`, `cargo test`, `cargo audit --format sarif`.

## Benchmark profiling

Use `.codex/skills/jaeger-spotbugs-benchmark/SKILL.md` for Jaeger-based performance profiling of SpotBugs benchmark traces. Requires Docker and reports bottlenecks by rule, jar, and class.

## No-Go resume

Use `prompts/authoring-no-go-resume.md` only when resuming a prior external No-Go PR. This imports plan/spec/impl from the source PR, fixes issues, and updates `prompts/references/no-go-history.md`.
