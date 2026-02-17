---
name: jaeger-trace-screenshot
description: Capture a Jaeger trace screenshot for a specified trace ID via `scripts/capture-jaeger-trace-screenshot.mjs`. Use when a trace ID is already known and the task is only to capture and return a deterministic screenshot artifact path.
---

# jaeger trace screenshot

## Inputs
- Repository root containing `scripts/capture-jaeger-trace-screenshot.mjs`.
- Target trace ID (required).
- Node.js + npm available.
- Jaeger UI reachable at `http://localhost:16686` unless overridden.

## Workflow
1. Prepare Playwright runtime once per environment:
   - `npm install --no-save --no-package-lock playwright@1.53.0`
   - `npx playwright install --with-deps chromium`
2. Capture screenshot with explicit trace ID:
   - `screenshot_png="$(JAEGER_OUT_DIR=<out-dir> JAEGER_BASE_URL=<jaeger-base-url> node scripts/capture-jaeger-trace-screenshot.mjs <trace-id>)"`
3. Return the resolved screenshot path from `screenshot_png`.

## Defaults
- Use `JAEGER_OUT_DIR=target/bench` when output directory is not specified.
- Use `JAEGER_BASE_URL=http://localhost:16686` when base URL is not specified.

## Guardrails
- Do not replace the instructed trace ID with "latest trace" lookup unless explicitly asked.
- Keep output filename format as `jaeger-trace-<trace-id>.png`.
- Treat screenshot capture failure as blocking and surface the command error.
