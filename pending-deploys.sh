#!/usr/bin/env bash
#
# pending-deploys.sh — report which services have undeployed changes.
#
# Each `deploy-<service>` action in remote.ron writes the current time
# to `<service>/deployed` (via `date --rfc-3339=seconds > deployed`)
# immediately before building the release binary from the working
# tree. A service therefore has a *pending* deployment when its source
# has changed since that timestamp.
#
# For every `<service>/deployed` marker this script compares the
# recorded deploy time against the most recent source change, where the
# source is the service's own directory plus the in-repo crates it
# depends on (its `path = "../<crate>"` Cargo.toml entries, e.g.
# `common`) and the workspace manifest/lock. A change counts if either:
#   * a commit touching those paths is newer than the deploy time, or
#   * those paths have uncommitted (or new, untracked) edits — the
#     deploy builds from the working tree, so these ship too.
#
# Exit status is 0 when everything is up to date and 1 when at least
# one service is pending, so it can gate a deploy script or hook.

set -euo pipefail

cd "$(dirname "$0")"

# Resolve the in-repo source paths a service is built from: its own
# directory, any sibling crates it path-depends on, and the workspace
# manifest + lockfile (which affect every build).
service_paths() {
    local service=$1
    local -a paths=("$service")

    if [[ -f "$service/Cargo.toml" ]]; then
        # Pull out `path = "../<crate>"` dependencies; absolute paths
        # (e.g. the local sqlx fork) point outside the repo and are
        # skipped.
        local dep
        while IFS= read -r dep; do
            [[ -d "$dep" ]] && paths+=("$dep")
        done < <(sed -nE 's/.*path *= *"(\.\.\/[^"]+)".*/\1/p' "$service/Cargo.toml" \
                 | sed 's#^\.\./##' | sort -u)
    fi

    [[ -f Cargo.toml ]] && paths+=("Cargo.toml")
    [[ -f Cargo.lock ]] && paths+=("Cargo.lock")

    printf '%s\n' "${paths[@]}"
}

pending=0
checked=0

# Iterate services in a stable order.
while IFS= read -r marker; do
    service=$(dirname "$marker")
    service=${service#./}
    checked=$((checked + 1))

    deployed_raw=$(<"$marker")
    deployed_raw=${deployed_raw//$'\n'/}

    # An empty or unparseable marker means we can't trust that a deploy
    # ever succeeded — treat it as pending and say so.
    if [[ -z "${deployed_raw// /}" ]] || ! deployed_ts=$(date -d "$deployed_raw" +%s 2>/dev/null); then
        printf 'PENDING  %-20s (no valid deploy timestamp: %q)\n' "$service" "$deployed_raw"
        pending=$((pending + 1))
        continue
    fi

    mapfile -t paths < <(service_paths "$service")

    # Newest commit touching any of the build's source paths.
    last_commit_ts=$(git log -1 --format=%ct -- "${paths[@]}" 2>/dev/null || true)

    # Uncommitted or untracked (non-ignored) edits to those paths.
    if [[ -n "$(git status --porcelain -- "${paths[@]}" 2>/dev/null)" ]]; then
        uncommitted=1
    else
        uncommitted=0
    fi

    reasons=()
    if [[ -n "$last_commit_ts" && "$last_commit_ts" -gt "$deployed_ts" ]]; then
        reasons+=("new commit $(date -d "@$last_commit_ts" '+%Y-%m-%d %H:%M')")
    fi
    if [[ "$uncommitted" -eq 1 ]]; then
        reasons+=("uncommitted changes")
    fi

    if [[ ${#reasons[@]} -gt 0 ]]; then
        reason_str=$(printf '%s, ' "${reasons[@]}")
        printf 'PENDING  %-20s deployed %s — %s\n' \
            "$service" \
            "$(date -d "@$deployed_ts" '+%Y-%m-%d %H:%M')" \
            "${reason_str%, }"
        pending=$((pending + 1))
    else
        printf 'current  %-20s deployed %s\n' \
            "$service" "$(date -d "@$deployed_ts" '+%Y-%m-%d %H:%M')"
    fi
done < <(find . -maxdepth 2 -name deployed -type f | sort)

echo
if [[ "$pending" -gt 0 ]]; then
    echo "$pending of $checked service(s) pending deployment."
    exit 1
else
    echo "All $checked service(s) up to date."
fi
