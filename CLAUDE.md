# CLAUDE.md

## Project guidelines

Read `AGENTS.md` for the authoritative project guidelines including:
- Project context, coding decisions, and commit style (Conventional Commits v1.0.0)
- Test harness guidelines (generic naming, Java 21 via `JAVA_HOME`)
- Plan management (`plans/` directory, `.done.md` rename convention)
- Release checklist

## Build & test commands

```bash
cargo fmt                        # format after every code change
cargo build
cargo test                       # needs JAVA_HOME pointing to Java 21
cargo audit --format sarif       # install: cargo install cargo-audit --locked
```

## Rule authoring workflow

For end-to-end rule development, follow `prompts/authoring-rule.md` as the orchestration prompt. It defines a phased subagent workflow:

| Phase | Prompt | Skill (`.codex/skills/`) |
|-------|--------|--------------------------|
| 1. Ideation | `prompts/ideate-rule.md` | - |
| 2. Plan | `prompts/authoring-plan.md` | `inspequte-rule-plan/SKILL.md` |
| 3. Spec | `prompts/authoring-spec.md` | `inspequte-rule-spec/SKILL.md` |
| 4. Impl | `prompts/authoring-impl.md` | `inspequte-rule-impl/SKILL.md` |
| 5. Verify | `prompts/authoring-verify.md` | `inspequte-rule-verify/SKILL.md` |
| Resume | `prompts/authoring-no-go-resume.md` | `inspequte-rule-no-go-resume/SKILL.md` |

Each prompt file contains the full instructions for its phase. Each skill file under `.codex/skills/` contains the detailed workflow, guardrails, template snippets, and definition of done. Read the relevant prompt and skill when executing a phase.

## Benchmark profiling

See `.codex/skills/jaeger-spotbugs-benchmark/SKILL.md` for Jaeger-based performance profiling.
