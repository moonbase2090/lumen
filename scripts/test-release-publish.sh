#!/usr/bin/env bash
# Structure checks on .github/workflows/release-check.yml: the signing, notarizing
# and publishing path cannot be weakened without one of these failing. They read
# the workflow only; nothing is run or published.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec ruby -ryaml - "$ROOT/.github/workflows/release-check.yml" <<'RUBY'
w = YAML.safe_load(File.read(ARGV[0]), aliases: false)
jobs = w.fetch("jobs")
steps = ->(j) { jobs.fetch(j).fetch("steps", []) }
runs = ->(j) { steps.(j).map { |s| s["run"].to_s }.join("\n") }
failed = false
check = lambda do |desc, ok|
  puts "#{ok ? 'ok  ' : 'FAIL'} #{desc}"
  failed ||= !ok
end

check.("the workflow default permission is contents: read", w["permissions"] == { "contents" => "read" })
check.("only publish-preview has write access",
       jobs.select { |_, v| v.dig("permissions", "contents") == "write" }.keys == ["publish-preview"])
check.("no job asks for any other write permission",
       jobs.values.all? { |j| j.fetch("permissions", {}).all? { |k, v| k == "contents" || v != "write" } })
check.("publish-preview waits for the release gate and the signed build",
       jobs["publish-preview"]["needs"].sort == %w[macos-signed release-check])
cond = jobs["publish-preview"]["if"].to_s
check.("publish-preview runs only for a pushed preview tag",
       cond.include?("refs/tags/v") && cond.include?("-preview.") && cond.include?("github.event_name == 'push'"))
pub = runs.("publish-preview")
check.("the release is a prerelease and needs an existing tag",
       pub.include?("gh release create") && pub.include?("--prerelease") && pub.include?("--verify-tag"))
check.("nothing creates or pushes a tag or publishes a crate",
       jobs.keys.none? { |j| ["git tag", "git push", "--target", "cargo publish"].any? { |x| runs.(j).include?(x) } })
check.("the release text is checked before publishing",
       pub.index("check-public-text.sh") && pub.index("gh release create") &&
       pub.index("check-public-text.sh") < pub.index("gh release create"))
sign = runs.("macos-signed")
check.("the signing job notarizes the app and the disk image", sign.scan("scripts/notarize-macos.sh").size == 2)
order = ["notarize-macos.sh release-set", "build-bright-dmg.sh", "sign-macos-binaries.sh --identifier-prefix app.brightterm.Bright \"$DMG\"", "notarize-macos.sh \"$DMG\""].map { |x| sign.index(x) }
check.("the disk image is built from the stapled app, then signed, then notarized",
       order.none?(&:nil?) && order == order.sort)
check.("no signing or notarizing step can be skipped or ignored",
       %w[macos-signed publish-preview].all? do |j|
         steps.(j).none? { |s| s.key?("continue-on-error") || s["run"].to_s.include?("|| true") || s.key?("if") }
       end)
check.("the signing job has no unsigned fallback", !sign.include?("--adhoc"))
check.("the signing secrets come from repository secrets only",
       steps.("macos-signed").all? do |s|
         s.fetch("env", {}).select { |k, _| k.start_with?("APPLE_") }.values.all? { |v| v.to_s.start_with?("${{ secrets.") }
       end)
check.("no step expands a tag name inside a script",
       steps.("publish-preview").none? { |s| s["run"].to_s.include?("github.ref_name") })
check.("every action in the job with write access is pinned to a full commit",
       steps.("publish-preview").all? { |s| !s.key?("uses") || s["uses"].split("@")[1].to_s.size == 40 })
exit(failed ? 1 : 0)
RUBY
