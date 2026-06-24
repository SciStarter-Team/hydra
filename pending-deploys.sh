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
#
# With -d/--deploy it doesn't just report: it runs the needed deploys
# via the `remote` tool, skipping services that are already up to date.
# Deploys run in sequence in dependency order — log-service first,
# user-service second, frontend last — with any other pending services
# in between (their relative order matters less). The run stops at the
# first failed deploy.

set -euo pipefail

cd "$(dirname "$0")"

usage() {
    cat <<'EOF'
Usage: pending-deploys.sh [-d|--deploy]

Without options, report which services have undeployed changes (exit 1
if any are pending, 0 otherwise).

  -d, --deploy   Deploy the pending services via the `remote` tool, in
                 order: log-service, user-service, any other pending
                 services, then frontend last. Up-to-date services are
                 skipped.
  -h, --help     Show this help and exit.
EOF
}

do_deploy=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        -d|--deploy) do_deploy=1 ;;
        -h|--help) usage; exit 0 ;;
        *)
            printf 'pending-deploys.sh: unknown option: %s\n\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

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
pending_services=()

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
        pending_services+=("$service")
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
        pending_services+=("$service")
    else
        printf 'current  %-20s deployed %s\n' \
            "$service" "$(date -d "@$deployed_ts" '+%Y-%m-%d %H:%M')"
    fi
done < <(find . -maxdepth 2 -name deployed -type f | sort)

echo
if [[ "$pending" -eq 0 ]]; then
    echo "All $checked service(s) up to date."
    exit 0
fi

echo "$pending of $checked service(s) pending deployment."

# Without --deploy this is just a report; signal pending state for callers.
if [[ "$do_deploy" -eq 0 ]]; then
    exit 1
fi

# Is the named service in the pending set?
is_pending() {
    local target=$1 svc
    for svc in "${pending_services[@]}"; do
        [[ "$svc" == "$target" ]] && return 0
    done
    return 1
}

# Build the deploy order: log-service first, user-service second,
# frontend last, with any other pending services in between.
deploy_order=()
for svc in log-service user-service; do
    is_pending "$svc" && deploy_order+=("$svc")
done
for svc in "${pending_services[@]}"; do
    case "$svc" in
        log-service|user-service|frontend) ;;
        *) deploy_order+=("$svc") ;;
    esac
done
is_pending frontend && deploy_order+=("frontend")

echo
echo "Deploying ${#deploy_order[@]} service(s) via remote: ${deploy_order[*]}"

for service in "${deploy_order[@]}"; do
    # Only per-service binary deploys are automated, named deploy-<service>
    # after the directory holding the `deployed` marker. Special actions with
    # no marker — notably the location-service ip2location data swap
    # (deploy-location-service-data) — are never produced here and must be run
    # explicitly with `remote deploy-location-service-data`.
    action="deploy-$service"
    echo
    echo "==> $service ($action)"
    if remote "$action"; then
        echo "    $service deployed."
    else
        status=$?
        printf '    %s FAILED (remote exited %d) — stopping before remaining deploys.\n' \
            "$service" "$status" >&2
        exit 1
    fi
done

echo
echo "Deployed ${#deploy_order[@]} service(s) successfully."
