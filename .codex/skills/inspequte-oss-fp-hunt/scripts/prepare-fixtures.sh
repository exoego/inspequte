#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
workdir="${repo_root}/target/oss-fp/workdir"
report_dir="${repo_root}/target/oss-fp"
mkdir -p "${workdir}" "${report_dir}"

clone_if_missing() {
  local name="$1"
  local url="$2"

  if [ ! -d "${workdir}/${name}/.git" ]; then
    git clone --depth 1 "${url}" "${workdir}/${name}"
  fi
}

clone_if_missing "plasmo-config" "https://github.com/plasmoapp/plasmo-config.git"
clone_if_missing "okhttp-eventsource" "https://github.com/launchdarkly/okhttp-eventsource.git"

{
  echo "fixture\tsha"
  printf 'plasmo-config\t%s\n' "$(git -C "${workdir}/plasmo-config" rev-parse HEAD)"
  printf 'okhttp-eventsource\t%s\n' "$(git -C "${workdir}/okhttp-eventsource" rev-parse HEAD)"
} > "${report_dir}/fixture-shas.tsv"

echo "prepared fixtures in ${workdir}"
