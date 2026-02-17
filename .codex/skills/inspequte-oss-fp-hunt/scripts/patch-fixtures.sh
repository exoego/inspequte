#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
workdir="${repo_root}/target/oss-fp/workdir"
inspequte_root="${INSPEQUTE_REPO_ROOT:-${repo_root}}"

patch_settings_kts() {
  local file="$1"
  if ! rg -q "includeBuild\(.*gradle-plugin" "${file}"; then
    {
      cat <<EOT
pluginManagement {
    includeBuild(file(System.getenv("INSPEQUTE_REPO_ROOT") ?: "${inspequte_root}").resolve("gradle-plugin"))
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

EOT
      cat "${file}"
    } > "${file}.tmp"
    mv "${file}.tmp" "${file}"
  fi

  perl -0777 -i -pe 's/\}\s*rootProject/}\n\nrootProject/g' "${file}"
}

patch_settings_groovy() {
  local file="$1"
  if ! rg -q "includeBuild\(.*gradle-plugin" "${file}"; then
    {
      cat <<EOT
pluginManagement {
    includeBuild(new File(System.getenv("INSPEQUTE_REPO_ROOT") ?: "${inspequte_root}", "gradle-plugin"))
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

EOT
      cat "${file}"
    } > "${file}.tmp"
    mv "${file}.tmp" "${file}"
  fi

  perl -0777 -i -pe 's/\}\s*rootProject/}\n\nrootProject/g' "${file}"
}

patch_build_kts() {
  local file="$1"

  if ! rg -q 'id\("io.github.kengotoda.inspequte"\)' "${file}"; then
    perl -0777 -i -pe 's/plugins\s*\{\n/plugins {\n    id("io.github.kengotoda.inspequte")\n/s' "${file}"
  fi

  if ! rg -q '^inspequte\s*\{' "${file}"; then
    cat >> "${file}" <<'EOT'

inspequte {
    otel.set(System.getenv("INSPEQUTE_OTEL") ?: "http://localhost:4318/")
}
EOT
  fi
}

patch_settings_kts "${workdir}/plasmo-config/settings.gradle.kts"
patch_build_kts "${workdir}/plasmo-config/build.gradle.kts"

patch_settings_groovy "${workdir}/okhttp-eventsource/settings.gradle"
patch_build_kts "${workdir}/okhttp-eventsource/build.gradle.kts"

perl -0777 -i -pe 's#distributionUrl=.*#distributionUrl=https\\://services.gradle.org/distributions/gradle-8.10.2-bin.zip#' \
  "${workdir}/okhttp-eventsource/gradle/wrapper/gradle-wrapper.properties"

echo "patched fixture builds for inspequte plugin"
