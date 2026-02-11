#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rules_root="$repo_root/src/rules"
docs_root="$repo_root/docs/rules"

mkdir -p "$docs_root"
find "$docs_root" -maxdepth 1 -type f -name "*.md" -delete

mapfile -t spec_paths < <(find "$rules_root" -mindepth 2 -maxdepth 2 -type f -name "spec.md" | LC_ALL=C sort)

if [[ ${#spec_paths[@]} -eq 0 ]]; then
  echo "No spec.md files found under $rules_root" >&2
  exit 1
fi

{
  echo "# Rule Documentation"
  echo
  echo "Generated from \`src/rules/*/spec.md\` by \`scripts/generate-rule-docs.sh\`."
  echo
} > "$docs_root/index.md"

for spec_path in "${spec_paths[@]}"; do
  rule_id="$(basename "$(dirname "$spec_path")")"
  output_path="$docs_root/$rule_id.md"

  title="$(awk '/^#/{line=$0; sub(/^#+[[:space:]]*/, "", line); print line; exit}' "$spec_path")"
  if [[ -z "$title" ]]; then
    title="$rule_id"
  fi

  {
    printf "<!-- Generated from src/rules/%s/spec.md. Do not edit directly. -->\n\n" "$rule_id"
    cat "$spec_path"
    printf "\n"
  } > "$output_path"

  printf -- "- [%s](./%s.md) - %s\n" "$rule_id" "$rule_id" "$title" >> "$docs_root/index.md"
done

echo "Generated $docs_root (rules: ${#spec_paths[@]})."
