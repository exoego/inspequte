# Plan: OSS False-Positive Hunting Skill

## Objective
Create a Codex skill that reproducibly finds false positives (FPs) by running `inspequte` on a small set of OSS libraries, capturing Jaeger traces, and triaging every finding against source code.

## Background
Current rule validation is mostly fixture-driven. We need a repeatable OSS-based workflow to detect:
- Kotlin-bytecode-related FPs (compiler-generated patterns).
- JSpecify/Guava-related FPs (nullable contracts and generic propagation).

The skill should automate setup and execution, while keeping the final FP judgment explicit and reviewable.

## Target OSS Fixtures
- Kotlin-oriented fixture: `plasmoapp/plasmo-config`.
- Java + Guava fixture: `launchdarkly/okhttp-eventsource`.

Selection rationale:
- Both are compact libraries with manageable finding volume.
- Both are OSS and easy to clone in CI/local environments.
- `okhttp-eventsource` includes Guava and JSpecify-related usage paths needed for nullness validation.

Fallback policy:
- If either project stops building, replace with another small Gradle-based library meeting the same category constraints, and record the replacement and reason in the run report.

## Implementation Approach

### 1. Skill scaffold
- Create `.codex/skills/inspequte-oss-fp-hunt/SKILL.md`.
- Add `agents/openai.yaml` via `skill-creator` scripts for discoverability.
- Keep SKILL instructions concise and workflow-first; move verbose operational details into `references/` and deterministic steps into `scripts/`.

### 2. Repository bootstrap scripts
- Add script(s) under `.codex/skills/inspequte-oss-fp-hunt/scripts/` to:
  - Clone/update fixture repos under `target/oss-fp/`.
  - Pin/record commit SHA per run.
  - Ensure `JAVA_HOME` points to Java 21.
  - Build local `inspequte` CLI once (`cargo build`) and expose it on `PATH`.

### 3. Enable `inspequte` Gradle plugin in target repos
- Patch each cloned repo in a reversible local branch/worktree:
  - Add plugin application:
    - `id("io.github.kengotoda.inspequte")` (or Groovy DSL equivalent).
  - Configure plugin block:
    - `inspequte { otel.set("http://localhost:4318/") }` for traced runs.
  - Register execution through `check` (default plugin behavior) and/or explicit `inspequte` tasks.
- Prefer composite-build plugin resolution for local development:
  - `pluginManagement { includeBuild("<repo>/gradle-plugin") }`
  - Avoid publishing a temporary plugin artifact.
- For legacy Gradle builds (notably `guava-retrying`), include a compatibility patch path in the skill:
  - modernize wrapper/build script minimally so `inspequte` tasks can run.

### 4. Run analysis and collect SARIF
- Execute `./gradlew check` or targeted `inspequte*` tasks in each fixture.
- Collect SARIF outputs into:
  - `target/oss-fp/<project>/inspequte/<sourceSet>/report.sarif`.
- Normalize file paths in reports for stable diffing across runs.

### 5. Jaeger performance capture (full screenshot)
- Reuse the procedure from `.codex/skills/jaeger-spotbugs-benchmark/SKILL.md`.
- Required UI interaction before screenshot:
  - Identify `Expand +1`, then click the next sibling control exactly once via XPath:
    - `//*[@id='jaeger-ui-root']//*[contains(@class,'TimelineCollapser--btn-expand')]`
    - `(//*[@id='jaeger-ui-root']//*[contains(@class,'TimelineCollapser--btn-expand')])[1]/following-sibling::*[contains(@class,'TimelineCollapser--btn')][1]`
  - This collapses one level and reduces timeline height for usable full-page screenshots.
- After the click, capture full-page screenshot to:
  - `target/oss-fp/<project>/jaeger/jaeger-trace-<trace-id>.png`.
- Export trace JSON and compute bottlenecks (slowest rule/jar/class) with timing evidence.

### 6. FP triage workflow (finding-by-finding)
- Parse SARIF results and create a triage ledger:
  - `target/oss-fp/triage/<project>.md`.
- For each finding:
  - Open referenced source location in the target repo.
  - Compare finding to rule spec intent.
  - Classify: `TP`, `FP`, or `NeedsRuleSpecDecision`.
  - Add short evidence (code snippet summary + rule/message mismatch reason).
  - Tag likely cause (`kotlin-generated`, `jspecify-flow`, `classpath`, `other`).

### 7. Final run report
- Generate `target/oss-fp/report.md` containing:
  - Fixture SHAs and local patches applied.
  - Analysis runtime summary with Jaeger artifacts.
  - FP inventory by rule ID and root-cause tag.
  - Concrete follow-up items (rule fix, spec clarification, test addition).

## Test Cases
- Skill dry-run clones both fixtures and applies plugin changes without manual edits.
- Gradle execution emits SARIF for each targeted source set.
- Jaeger trace capture succeeds and screenshot exists after `Collapse +1` click.
- Triage ledger includes every SARIF result exactly once.
- Re-running the workflow updates artifacts deterministically (same input SHA => stable summary structure).

## Success Criteria
- One skill invocation can execute the end-to-end flow for both fixture categories.
- Jaeger full screenshots are captured with timeline collapsed once, preventing oversized unreadable traces.
- Every finding is reviewed with code-backed TP/FP classification.
- At least one actionable FP candidate (or explicit “none found” evidence) is produced per run.

## Dependencies
- Java 21 (`JAVA_HOME` configured).
- Rust toolchain and `cargo`.
- Docker (Jaeger all-in-one).
- Playwright/browser automation capability for Jaeger UI.
- Network access to GitHub for fixture checkout.

## Complexity Estimate
High

## Post-mortem
- Went well: Scripted fixture preparation and patching made the workflow reproducible across reruns.
- Tricky: Jaeger UI control selection was brittle; selecting the button immediately after `Expand +1` was required for stable screenshots.
- Follow-up: Keep report guidance aligned with scope policy (`--classpath` out of finding scope) and update FP summary heuristics from current run data.
