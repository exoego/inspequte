---
name: inspequte-oss-fp-hunt
description: Run inspequte false-positive hunting against small OSS libraries (Kotlin and Java+Guava), enable inspequte-gradle-plugin in cloned fixtures, capture Jaeger trace screenshots after clicking the button immediately after Expand +1, and triage SARIF findings one by one.
---

# inspequte OSS FP hunt

## Inputs
- Repository root of this project (contains `gradle-plugin/` and `target/debug/inspequte`).
- Docker available for Jaeger.
- Java 21 available and set via `JAVA_HOME`.
- Network access to clone OSS fixtures from GitHub.

## Outputs
- Fixture clones and temporary patches under `target/oss-fp/workdir/`.
- SARIF reports under `target/oss-fp/<fixture>/inspequte/`.
- Triage ledgers under `target/oss-fp/triage/`.
- Run summary in `target/oss-fp/report.md`.
- Jaeger screenshot and trace analysis under `target/oss-fp/jaeger/`.

## Fixture policy
Default fixture set:
- Kotlin fixture: `plasmoapp/plasmo-config`
- Java+Guava fixture: `launchdarkly/okhttp-eventsource`

If one fixture is not buildable, replace it with another thin OSS fixture of the same category and record the reason in `target/oss-fp/report.md`.

## Workflow
1. Execute `.codex/skills/inspequte-oss-fp-hunt/scripts/run-once.sh` from repository root.
2. The script clones fixtures, enables `io.github.kengotoda.inspequte` with local `includeBuild`, runs analysis tasks, and exports SARIF + triage placeholders.
3. Open Jaeger UI and load latest trace for service `inspequte`.
4. Identify `Expand +1` using this XPath, then click the next sibling button once:
   - `//*[@id='jaeger-ui-root']//*[contains(@class,'TimelineCollapser--btn-expand')]`
   - `(//*[@id='jaeger-ui-root']//*[contains(@class,'TimelineCollapser--btn-expand')])[1]/following-sibling::*[contains(@class,'TimelineCollapser--btn')][1]`
5. Capture a full-page screenshot to `target/oss-fp/jaeger/jaeger-trace-<trace-id>.png`.
6. Export trace JSON and analyze with `.codex/skills/jaeger-spotbugs-benchmark/scripts/export-jaeger-trace.sh` and `analyze-trace-json.sh`.
7. Review findings one-by-one in each triage ledger and classify `TP` or `FP` with short code evidence.

## Finding triage rules
For each SARIF result:
- Confirm referenced file/line exists in fixture source.
- Compare finding message against actual code and expected rule intent.
- Mark:
  - `TP`: rule behavior matches real problem.
  - `FP`: finding is not actionable or contradicts code semantics.
- Add short reason and suggested rule/test follow-up.

## Guardrails
- Do not edit fixture repositories outside `target/oss-fp/workdir/`.
- Keep all artifacts under `target/oss-fp/`.
- Do not report performance bottlenecks without numeric evidence from trace summary.
- Always click the button immediately after `Expand +1` exactly once before full screenshot.
