#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/prepare-verify-input.sh <rule-id> [base-ref]

Arguments:
  <rule-id>   Rule directory under src/rules/<rule-id> (required)
  [base-ref]  Optional git ref for PR-style verification (example: origin/main)

Behavior:
  - Copies src/rules/<rule-id>/spec.md to verify-input/spec.md
  - Writes a patch to verify-input/diff.patch
  - Writes sorted changed files to verify-input/changed-files.txt
  - Copies changed files into verify-input/changes/
  - Creates verify-input/reports/ for build/test/audit outputs
EOF
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
  usage
  exit 1
fi

rule_id="$1"
base_ref="${2:-}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rule_spec="$repo_root/src/rules/$rule_id/spec.md"
verify_root="$repo_root/verify-input"
changes_root="$verify_root/changes"
reports_root="$verify_root/reports"
changed_files_path="$verify_root/changed-files.txt"
deleted_files_path="$verify_root/deleted-files.txt"
diff_path="$verify_root/diff.patch"

if [[ ! -f "$rule_spec" ]]; then
  echo "Missing rule spec: $rule_spec" >&2
  exit 1
fi

mkdir -p "$verify_root" "$changes_root" "$reports_root"
cp "$rule_spec" "$verify_root/spec.md"

if [[ -n "$base_ref" ]]; then
  git -C "$repo_root" rev-parse --verify "$base_ref" >/dev/null 2>&1
  git -C "$repo_root" diff --binary "$base_ref...HEAD" -- . ":(exclude)verify-input" > "$diff_path"
  git -C "$repo_root" diff --name-only "$base_ref...HEAD" -- . ":(exclude)verify-input" | LC_ALL=C sort > "$changed_files_path"
  git -C "$repo_root" diff --name-status "$base_ref...HEAD" -- . ":(exclude)verify-input" \
    | awk '$1 ~ /^D/ {print $2}' \
    | LC_ALL=C sort > "$deleted_files_path"
else
  git -C "$repo_root" diff --binary -- . ":(exclude)verify-input" > "$diff_path"
  git -C "$repo_root" diff --name-only -- . ":(exclude)verify-input" | LC_ALL=C sort > "$changed_files_path"
  git -C "$repo_root" diff --name-status -- . ":(exclude)verify-input" \
    | awk '$1 ~ /^D/ {print $2}' \
    | LC_ALL=C sort > "$deleted_files_path"
fi

find "$changes_root" -type f -delete

while IFS= read -r rel_path; do
  [[ -z "$rel_path" ]] && continue
  abs_path="$repo_root/$rel_path"
  if [[ -f "$abs_path" ]]; then
    mkdir -p "$changes_root/$(dirname "$rel_path")"
    cp "$abs_path" "$changes_root/$rel_path"
  fi
done < "$changed_files_path"

cat > "$verify_root/README.md" <<'EOF'
# verify-input

`verify-input/` is the only input directory for isolated verification.

Files:
- `spec.md`: copied rule contract used for verification.
- `diff.patch`: patch under review.
- `changed-files.txt`: sorted list of changed files in the patch.
- `deleted-files.txt`: sorted list of deleted files.
- `changes/`: current snapshots of changed files.
- `reports/`: command outputs used as verification evidence.

Populate reports before running verify, for example:

```bash
cargo build > verify-input/reports/cargo-build.txt 2>&1
cargo test > verify-input/reports/cargo-test.txt 2>&1
cargo audit --format sarif > verify-input/reports/cargo-audit.sarif
```
EOF

echo "Prepared $verify_root for rule '$rule_id'."
if [[ -n "$base_ref" ]]; then
  echo "Diff source: $base_ref...HEAD"
else
  echo "Diff source: working tree vs HEAD"
fi
echo "Changed files copied into verify-input/changes/."
echo "Add reports into verify-input/reports/ before running verify."
