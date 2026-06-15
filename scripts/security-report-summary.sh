#!/usr/bin/env bash
set -euo pipefail

tool="${1:?usage: security-report-summary.sh <tool> <report>}"
report="${2:?usage: security-report-summary.sh <tool> <report>}"

printf '::group::%s report summary\n' "${tool}"

if [ ! -s "${report}" ]; then
  printf 'No report was written at %s.\n' "${report}"
  printf '::endgroup::\n'
  exit 0
fi

case "${tool}" in
  cargo-audit)
    jq -r '
      def item:
        .advisory as $advisory
        | .package as $package
        | "- \($advisory.id // "unknown"): \($package.name // "unknown") \($package.version // "") - \($advisory.title // "no title")";

      [(.vulnerabilities.list // [])[] | item] as $items
      | if ($items | length) == 0 then
          "No cargo-audit advisories reported."
        else
          "cargo-audit advisories (\($items | length)):\n" + ($items | join("\n"))
        end
    ' "${report}"
    ;;
  cargo-deny)
    jq -sr '
      def diagnostic:
        "- "
        + ((.level // .severity // "diagnostic") | tostring)
        + ": "
        + (.message // .reason // .title // (. | tostring));

      [.. | objects | select(has("message") or has("reason") or has("title")) | diagnostic] as $items
      | if ($items | length) == 0 then
          "No cargo-deny diagnostics reported."
        else
          "cargo-deny diagnostics (\($items | length)):\n" + ($items[0:25] | join("\n"))
        end
    ' "${report}"
    ;;
  osv-scanner)
    jq -r '
      def vuln($package):
        "- "
        + (.id // "unknown")
        + ": "
        + ($package.package.name // $package.package.purl // "unknown package")
        + " - "
        + (.summary // "no summary");

      [(.results // [])[]
        | (.packages // [])[] as $package
        | ($package.vulnerabilities // [])[]
        | vuln($package)] as $items
      | if ($items | length) == 0 then
          "No OSV vulnerabilities reported."
        else
          "OSV vulnerabilities (\($items | length)):\n" + ($items | join("\n"))
        end
    ' "${report}"
    ;;
  semgrep)
    jq -r '
      def result:
        . as $result
        | ($result.locations[0].physicalLocation.artifactLocation.uri // "unknown file") as $path
        | ($result.locations[0].physicalLocation.region.startLine // "?") as $line
        | "- "
        + ($result.ruleId // "unknown-rule")
        + " at "
        + $path
        + ":"
        + ($line | tostring)
        + " - "
        + ($result.message.text // "no message");

      [(.runs // [])[] | (.results // [])[] | result] as $items
      | if ($items | length) == 0 then
          "No Semgrep findings reported."
        else
          "Semgrep findings (\($items | length)):\n" + ($items | join("\n"))
        end
    ' "${report}"
    ;;
  trivy)
    jq -r '
      def vuln($target):
        "- vulnerability "
        + (.VulnerabilityID // "unknown")
        + " ["
        + (.Severity // "unknown")
        + "] in "
        + $target
        + ": "
        + (.PkgName // "unknown package")
        + " "
        + (.InstalledVersion // "")
        + " - "
        + (.Title // "no title");
      def misconfig($target):
        "- misconfiguration "
        + (.ID // "unknown")
        + " ["
        + (.Severity // "unknown")
        + "] in "
        + $target
        + ": "
        + (.Title // .Message // "no title");
      def secret($target):
        "- secret "
        + (.RuleID // "unknown")
        + " ["
        + (.Severity // "unknown")
        + "] in "
        + $target
        + ": "
        + (.Title // "no title");

      [(.Results // [])[]
        | .Target as $target
        | ((.Vulnerabilities // [])[] | vuln($target)),
          ((.Misconfigurations // [])[] | misconfig($target)),
          ((.Secrets // [])[] | secret($target))] as $items
      | if ($items | length) == 0 then
          "No Trivy findings reported."
        else
          "Trivy findings (\($items | length)):\n" + ($items | join("\n"))
        end
    ' "${report}"
    ;;
  *)
    printf 'Unknown security report tool: %s\n' "${tool}" >&2
    printf '::endgroup::\n'
    exit 1
    ;;
esac

printf '::endgroup::\n'
