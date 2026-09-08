#!/usr/bin/env bash
# The vendored termlens skill must name the version this repository depends on.
#
# `.claude/skills/termlens/SKILL.md` is a copy of the file termlens ships for
# coding agents. It is refreshed by hand, and the failure mode is silent: the
# dev-dependency gets bumped, the copy does not, and every agent working in
# this repository is then handed guidance for a version that is no longer here
# — wrong signatures, absent APIs, advice that was true one release ago. The
# 0.9 copy taught `assert_screen_snapshot!(screen)` and `mouse_mode()`, both
# of which 0.10 changed, and nothing in CI noticed.
#
# Nothing can diff it against upstream: the published crate does not ship the
# skill, so there is no registry copy to compare with. What *is* checkable is
# that the two versions agree, which is exactly the drift that happens.
#
# Compares major.minor only. A termlens patch release does not rewrite the
# skill, and demanding a re-copy for every one of them would make this noise.
#
# Usage: check-skill-version.sh [SKILL.md] [Cargo.toml]
set -euo pipefail

skill="${1:-.claude/skills/termlens/SKILL.md}"
# termlens is a dev-dependency of exactly one crate, not a workspace
# dependency (see CONTRIBUTING.md: it pairs with nothing and moves alone).
manifest="${2:-crates/oxmera-cli/Cargo.toml}"

[ -f "$skill" ] || { echo "::error::$skill does not exist"; exit 1; }
[ -f "$manifest" ] || { echo "::error::$manifest does not exist"; exit 1; }

# "Written against **termlens 0.10.1**." -> 0.10
skill_version="$(sed -n 's/.*Written against \*\*termlens \([0-9][0-9.]*\)\*\*.*/\1/p' "$skill" | head -1)"
[ -n "$skill_version" ] || {
  echo "::error::$skill has no 'Written against **termlens X.Y.Z**' line to check"
  exit 1
}
skill_minor="$(echo "$skill_version" | cut -d. -f1,2)"

# Both spellings, because either is a legitimate way to write the dependency:
#   termlens = { version = "0.10", features = ["serde"] }   ->  0.10
#   termlens = "0.10"                                       ->  0.10
dep_version="$(sed -n \
  -e 's/^termlens = .*version = "\([0-9][0-9.]*\)".*/\1/p' \
  -e 's/^termlens = "\([0-9][0-9.]*\)".*/\1/p' \
  "$manifest" | head -1)"
[ -n "$dep_version" ] || {
  echo "::error::no termlens dependency with a version found in $manifest"
  exit 1
}
dep_minor="$(echo "$dep_version" | cut -d. -f1,2)"

if [ "$skill_minor" != "$dep_minor" ]; then
  echo "::error::the vendored termlens skill is written against ${skill_version} but this repository depends on ${dep_version}."
  echo "::error::Refresh it: cp ../termlens/skills/termlens/SKILL.md ${skill}"
  exit 1
fi

echo "the vendored termlens skill (${skill_version}) matches the dependency (${dep_version})"

# The same drift, one layer out. The termlens `report` action takes the
# termlens-cli version as a literal in the workflow files, and a literal beside
# a dependency is a pin that goes stale silently: the suite would then be
# rendered by a tool from a different release than the library that produced
# the screens. Nothing else compares the two, so this does.
cli_pins="$(grep -rhoE 'cli-version: *"[0-9][0-9.]*"' .github/workflows/ 2>/dev/null | grep -oE '[0-9][0-9.]+' | sort -u)"
if [ -n "$cli_pins" ]; then
  for pin in $cli_pins; do
    pin_minor="$(echo "$pin" | cut -d. -f1,2)"
    if [ "$pin_minor" != "$dep_minor" ]; then
      echo "::error::a workflow pins termlens-cli ${pin} but this repository depends on termlens ${dep_version}."
      echo "::error::Bump every 'cli-version:' under .github/workflows/ to match."
      exit 1
    fi
  done
  echo "the termlens-cli pins in .github/workflows ($(echo "$cli_pins" | tr '\n' ' ')) match the dependency (${dep_version})"
fi
