#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
export INSPEQUTE_REPO_ROOT="${repo_root}"
export INSPEQUTE_OTEL="${INSPEQUTE_OTEL:-http://localhost:4318/}"

if [ -z "${JAVA_HOME:-}" ]; then
  if command -v /usr/libexec/java_home >/dev/null 2>&1; then
    export JAVA_HOME="$(/usr/libexec/java_home -v 21)"
  fi
fi

if [ -z "${JAVA_HOME:-}" ] || [ ! -d "${JAVA_HOME}" ]; then
  echo "JAVA_HOME for Java 21 is required" >&2
  exit 1
fi

mkdir -p "${repo_root}/target/oss-fp/logs" "${repo_root}/target/oss-fp/triage" "${repo_root}/target/oss-fp/jaeger"

"${repo_root}/.codex/skills/inspequte-oss-fp-hunt/scripts/prepare-fixtures.sh"
"${repo_root}/.codex/skills/inspequte-oss-fp-hunt/scripts/patch-fixtures.sh"

(
  cd "${repo_root}"
  cargo build >/dev/null
)

export PATH="${repo_root}/target/debug:${PATH}"

"${repo_root}/.codex/skills/jaeger-spotbugs-benchmark/scripts/start-jaeger.sh" >/dev/null

run_fixture() {
  local name="$1"
  local dir="$2"
  local tasks="${3:-inspequteMain inspequteTest}"
  local log_file="${repo_root}/target/oss-fp/logs/${name}.log"
  local out_dir="${repo_root}/target/oss-fp/${name}"
  local triage_file="${repo_root}/target/oss-fp/triage/${name}.md"

  mkdir -p "${out_dir}"

  (
    cd "${dir}"
    ./gradlew --no-daemon clean ${tasks} >"${log_file}" 2>&1
  )

  rm -rf "${out_dir}/inspequte"
  cp -R "${dir}/build/inspequte" "${out_dir}/inspequte"

  {
    echo "# Triage: ${name}"
    echo
    echo "source: ${dir}"
    echo

    local total=0
    while IFS= read -r sarif; do
      local rel_sarif="${sarif#${dir}/}"
      echo "## ${rel_sarif}"
      local count
      count=$(jq '[.runs[]?.results[]?] | length' "${sarif}")
      echo "findings: ${count}"
      echo

      if [ "${count}" -eq 0 ]; then
        echo "No findings."
        echo
        continue
      fi

      jq -r '
        .runs[]?.results[]? |
        "- [ ] status=UNTRIAGED rule=\(.ruleId // "") file=\(.locations[0].physicalLocation.artifactLocation.uri // "") line=\(.locations[0].physicalLocation.region.startLine // 0) message=\((.message.text // "") | gsub("\\n"; " "))"
      ' "${sarif}"
      echo
      total=$((total + count))
    done < <(find "${out_dir}/inspequte" -type f -name 'report.sarif' | sort)

    echo "total-findings: ${total}"
  } > "${triage_file}"
}

run_fixture "plasmo-config" "${repo_root}/target/oss-fp/workdir/plasmo-config" "inspequteMain inspequteTest"
run_fixture "okhttp-eventsource" "${repo_root}/target/oss-fp/workdir/okhttp-eventsource" "inspequteMain"

triage_files=(
  "${repo_root}/target/oss-fp/triage/plasmo-config.md"
  "${repo_root}/target/oss-fp/triage/okhttp-eventsource.md"
)

read -r triage_total triage_untriaged triage_tp triage_fp dep_findings line0_findings kotlin_findings nullness_findings nullness_unique <<EOF
$(awk '
BEGIN {
  total = 0
  untriaged = 0
  tp = 0
  fp = 0
  dep = 0
  line0 = 0
  kotlin = 0
  nullness = 0
}
/status=/ { total++ }
/status=UNTRIAGED/ { untriaged++ }
/status=TP/ { tp++ }
/status=FP/ { fp++ }
/\.gradle\/caches\/modules-2/ { dep++ }
/ line=0 / { line0++ }
/\/build\/classes\/kotlin\// { kotlin++ }
/rule=NULLNESS/ {
  nullness++
  entry = $0
  sub(/^.*- \[ \] /, "", entry)
  unique_nullness[entry] = 1
}
END {
  unique_count = 0
  for (item in unique_nullness) {
    unique_count++
  }
  printf "%d %d %d %d %d %d %d %d %d\n", total, untriaged, tp, fp, dep, line0, kotlin, nullness, unique_count
}
' "${triage_files[@]}")
EOF

nullness_duplicate=$((nullness_findings - nullness_unique))
if [ "${nullness_duplicate}" -lt 0 ]; then
  nullness_duplicate=0
fi

assessment="preliminary"
if [ "${triage_total}" -eq 0 ]; then
  assessment="no-findings"
elif [ "${triage_untriaged}" -eq 0 ]; then
  assessment="triaged"
fi

fp_summary="Most FP risk appears to come from dependency bytecode scope and duplicate nullness reporting, not project-owned source."

fp_thoughts_json="$(
  jq -n \
    --arg assessment "${assessment}" \
    --arg summary "${fp_summary}" \
    --argjson total "${triage_total}" \
    --argjson untriaged "${triage_untriaged}" \
    --argjson tp "${triage_tp}" \
    --argjson fp "${triage_fp}" \
    --argjson dep "${dep_findings}" \
    --argjson line0 "${line0_findings}" \
    --argjson kotlin "${kotlin_findings}" \
    --argjson nullness "${nullness_findings}" \
    --argjson null_unique "${nullness_unique}" \
    --argjson null_dup "${nullness_duplicate}" \
    '{
      assessment: $assessment,
      summary: $summary,
      triage_status: {
        total_findings: $total,
        untriaged: $untriaged,
        tp: $tp,
        fp: $fp
      },
      fp_signals: {
        dependency_cache_findings: $dep,
        line_zero_locations: $line0,
        kotlin_generated_findings: $kotlin,
        nullness_findings: $nullness,
        unique_nullness_findings: $null_unique,
        duplicated_nullness_findings: $null_dup
      },
      final_thoughts: [
        {
          id: "dependency-scope-noise",
          confidence: (if $dep > 0 then "high" else "low" end),
          thought: "Many findings come from third-party jars in Gradle cache. These are likely non-actionable for OSS owner triage unless dependency scanning is explicitly desired."
        },
        {
          id: "line-zero-actionability",
          confidence: (if $line0 > 0 then "high" else "low" end),
          thought: "Class-level findings with line=0 are hard to action and should be considered FP candidates or downgraded in OSS workflows."
        },
        {
          id: "nullness-duplication-noise",
          confidence: (if $null_dup > 0 then "medium" else "low" end),
          thought: "Repeated NULLNESS alerts on the same target inflate noise and should be deduplicated by rule, file, line, and message."
        }
      ],
      recommended_followups: [
        "Exclude external dependency jars from default OSS FP-hunt scope or tag them as dependency findings.",
        "Deduplicate identical findings before report generation.",
        "For Kotlin bytecode, avoid duplicated reports for synthetic accessors/bridges and keep one actionable location."
      ]
    }'
)"

trace_json="$(${repo_root}/.codex/skills/jaeger-spotbugs-benchmark/scripts/export-jaeger-trace.sh)"
summary_line="$(${repo_root}/.codex/skills/jaeger-spotbugs-benchmark/scripts/analyze-trace-json.sh "${trace_json}" | tr '\n' '\t')"
trace_id="$(basename "${trace_json}" | sed -E 's/^jaeger-trace-(.+)\.json$/\1/')"

{
  echo "# OSS FP Hunt Report"
  echo
  echo "fixtures:"
  cat "${repo_root}/target/oss-fp/fixture-shas.tsv"
  echo
  echo "trace-id: ${trace_id}"
  echo "trace-json: ${trace_json}"
  echo "trace-summary: ${summary_line}"
  echo
  echo "triage-files:"
  for triage in "${triage_files[@]}"; do
    echo "- ${triage#${repo_root}/}"
  done
  echo
  echo "fp-final-thoughts-json:"
  echo '```json'
  echo "${fp_thoughts_json}"
  echo '```'
} > "${repo_root}/target/oss-fp/report.md"

echo "run complete"
echo "trace-id=${trace_id}"
echo "report=${repo_root}/target/oss-fp/report.md"
