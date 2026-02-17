---
name: jaeger-spotbugs-benchmark
description: Launch Jaeger, run SpotBugs benchmark traces, capture Jaeger UI screenshots via the shared `scripts/capture-jaeger-trace-screenshot.mjs` helper, and report bottlenecks by rule, jar, and class for inspequte. Use when asked to profile `scripts/bench-spotbugs.sh`, inspect Jaeger traces, save screenshots under `target/bench`, and explain the slowest components.
---

# jaeger spotbugs benchmark

## Inputs
- Repository root with `scripts/bench-spotbugs.sh`.
- Docker available locally.
- Jaeger UI reachable at `http://localhost:16686`.
- OTLP HTTP endpoint `http://localhost:4318/`.
- Node.js + npm available for screenshot capture.
- Execute all commands from repository root.

## Outputs
- Jaeger container running.
- Benchmark log in `target/bench/spotbugs.log`.
- Jaeger trace screenshot in `target/bench/`.
- Trace JSON export in `target/bench/`.
- Bottleneck report naming the slowest rule, jar, and class.

## Workflow
1. Start Jaeger:
   - Run `.codex/skills/jaeger-spotbugs-benchmark/scripts/start-jaeger.sh`.
2. Run benchmark with tracing enabled:
   - Run `.codex/skills/jaeger-spotbugs-benchmark/scripts/run-bench-spotbugs-with-otel.sh`.
   - Override repeat count with positional arg when needed.
   - Override endpoint with `OTEL_ENDPOINT` only when explicitly requested.
3. Export latest trace JSON and resolve trace ID:
   - `trace_json="$(.codex/skills/jaeger-spotbugs-benchmark/scripts/export-jaeger-trace.sh)"`
   - `trace_id="$(basename "${trace_json}" .json)"`
   - `trace_id="${trace_id#jaeger-trace-}"`
4. Prepare Playwright runtime once per environment:
   - `npm install --no-save --no-package-lock playwright@1.53.0`
   - `npx playwright install --with-deps chromium`
5. Capture screenshot via shared script:
   - `screenshot_png="$(JAEGER_OUT_DIR=target/bench node scripts/capture-jaeger-trace-screenshot.mjs "${trace_id}")"`
6. Analyze trace:
   - `.codex/skills/jaeger-spotbugs-benchmark/scripts/analyze-trace-json.sh "${trace_json}"`
7. Report bottleneck:
   - Include trace ID and screenshot path.
   - Identify one slowest rule, one slowest jar, and one slowest class.
   - Include concrete timing evidence from trace analysis.
   - Add a short investigation note tying screenshot timeline shape to the extracted slow spans.

## Reporting Template
Use this exact format:

```markdown
## Bottleneck Report
- Trace ID: <trace-id>
- Screenshot: target/bench/jaeger-trace-<trace-id>.png
- Slowest rule: <rule-id> (<total-ms> ms total across <span-count> spans)
- Slowest jar: <jar-path-or-name> (<total-ms> ms total across <span-count> spans)
- Slowest class: <class-name-or-entry> (<total-ms> ms total across <span-count> spans)
- Investigation:
  - Screenshot evidence: <what the expanded timeline shows>
  - Trace evidence: <why these three are the bottleneck>
  - Next action: <one concrete optimization direction>
```

## Guardrails
- Use the exact Jaeger container image: `jaegertracing/all-in-one:latest`.
- Keep screenshot files only under `target/bench`.
- Do not report a bottleneck without numeric timing evidence.
- Prefer Jaeger API export plus screenshot observation over guesswork.
