#!/usr/bin/env bash
#
# pending-deploys.sh — report which services have undeployed changes.
#
# Each `deploy-<service>` action in remote.ron writes a marker to
# `<service>/deployed` immediately before building the release binary
# from the working tree:
#
#     line 1: the deploy time (`date --rfc-3339=seconds`)
#     line 2: the deployed revision (`git rev-parse HEAD`)
#
# A service has a *pending* deployment when the source it is built from
# — its own directory plus the in-repo crates it path-depends on, and
# the workspace manifest/lock — differs from what that deploy shipped.
#
# With a recorded revision the check is content-based: the working tree
# (committed or not) is diffed against the deployed revision, and a
# differing file counts only if it was modified after the marker was
# stamped (the marker file's own mtime). That makes the check immune to
# commit-timestamp skew (pulled or rebased commits carry old dates) and
# stops flagging edits that were already in the working tree when the
# deploy built it. The shared Cargo.lock is handled precisely: a lock
# change only flags the services whose dependency closure contains a
# changed package. Old single-line markers (date only) fall back to the
# previous timestamp comparison until their next deploy upgrades them.
#
# Services that remote.ron knows how to deploy but that have no marker
# at all are reported as pending ("never deployed").
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
#
# Known limitation: the marker is stamped before the build, so a deploy
# that fails partway still looks deployed. And when the working tree was
# dirty at deploy time, the recorded revision can't describe exactly
# what shipped — the mtime gate approximates it (deciding by whether the
# file changed after the stamp), which errs toward "pending" after
# branch switches or checkouts that refresh mtimes.

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

# The service's own directory plus any sibling crates it path-depends
# on. The workspace manifest and lock are deliberately not included —
# they are shared, and handled with finer grain by the caller.
service_core_paths() {
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

    printf '%s\n' "${paths[@]}"
}

# Every file under the given paths whose working-tree content differs
# from the given revision: committed changes since the revision, dirty
# tracked files, and untracked (non-ignored) files.
changed_files() {
    local rev=$1; shift
    {
        git diff --name-only "$rev" -- "$@" 2>/dev/null || true
        git status --porcelain -uall -- "$@" 2>/dev/null \
            | awk '/^\?\?/ { print substr($0, 4) }'
    } | sort -u
}

# Was the file modified after the deploy stamped this marker? Missing
# files (deletions) can't be dated, so they count as changed.
newer_than_marker() {
    local file=$1 marker=$2
    [[ ! -e "$file" || "$file" -nt "$marker" ]]
}

# Emit "name version" per [[package]] entry of a Cargo.lock on stdin.
lock_packages() {
    awk -F'"' '/^name = /{n=$2} /^version = /{print n " " $2}'
}

# "name version" pairs whose lock entry differs between the given
# revision and the working tree (added, removed, or version-changed).
# Version-precise so that e.g. adding prost 0.14 for one crate doesn't
# implicate services still on an untouched prost 0.13 entry.
lock_changed_packages() {
    local rev=$1
    comm -3 \
        <(git show "$rev:Cargo.lock" 2>/dev/null | lock_packages | sort) \
        <(lock_packages < Cargo.lock | sort) \
        | sed 's/^\t//' | sort -u
}

# "name version" pairs for every package in the service's dependency
# closure, per the current lock. Fails when the closure can't be
# resolved (caller then falls back to the conservative answer).
service_closure() {
    local service=$1 pkg
    pkg=$(sed -nE 's/^name *= *"([^"]+)".*/\1/p' "$service/Cargo.toml" | head -1)
    [[ -n "$pkg" ]] || return 1
    cargo tree -p "$pkg" --prefix none --locked 2>/dev/null \
        | awk 'NF >= 2 { v = $2; sub(/^v/, "", v); print $1 " " v }' | sort -u
}

pending=0
checked=0
pending_services=()

# Services = every directory holding a `deployed` marker, plus every
# service remote.ron stamps one for (so a service that has never been
# deployed from this checkout still shows up).
services=$(
    {
        find . -maxdepth 2 -name deployed -type f | sed 's#^\./##; s#/deployed$##'
        sed -nE 's/.*cd ([A-Za-z0-9_-]+) && date --rfc-3339=seconds > deployed.*/\1/p' remote.ron
    } | sort -u
)

while IFS= read -r service; do
    [[ -n "$service" ]] || continue
    marker="$service/deployed"
    checked=$((checked + 1))

    if [[ ! -f "$marker" ]]; then
        printf 'PENDING  %-20s (never deployed — no %s marker)\n' "$service" "$marker"
        pending=$((pending + 1))
        pending_services+=("$service")
        continue
    fi

    deployed_raw=$(sed -n 1p "$marker")
    deployed_rev=$(sed -n 2p "$marker")
    deployed_ts=""
    if [[ -n "${deployed_raw// /}" ]]; then
        deployed_ts=$(date -d "$deployed_raw" +%s 2>/dev/null) || deployed_ts=""
    fi

    if [[ -n "$deployed_ts" ]]; then
        deployed_desc="deployed $(date -d "@$deployed_ts" '+%Y-%m-%d %H:%M')"
    else
        deployed_desc="deployed at ${deployed_rev:0:12}"
    fi

    mapfile -t core_paths < <(service_core_paths "$service")
    reasons=()

    if [[ "$deployed_rev" =~ ^[0-9a-f]{40}$ ]] \
        && git cat-file -e "$deployed_rev^{commit}" 2>/dev/null; then
        # Content-based check against the recorded deployed revision.

        # Source drift: files differing from the deployed revision that
        # changed after the deploy stamped the marker. (Files already in
        # their current state at deploy time shipped with it.)
        changed=()
        while IFS= read -r file; do
            [[ -n "$file" ]] || continue
            if newer_than_marker "$file" "$marker"; then
                changed+=("$file")
            fi
        done < <(changed_files "$deployed_rev" "${core_paths[@]}")

        if [[ ${#changed[@]} -gt 0 ]]; then
            sample=$(printf '%s, ' "${changed[@]:0:3}")
            sample=${sample%, }
            if [[ ${#changed[@]} -gt 3 ]]; then
                sample+=" +$((${#changed[@]} - 3)) more"
            fi
            reasons+=("${#changed[@]} source file(s) changed: $sample")
        fi

        # The workspace manifest affects every build (profiles, patches).
        if [[ -n "$(changed_files "$deployed_rev" Cargo.toml)" ]] \
            && newer_than_marker Cargo.toml "$marker"; then
            reasons+=("workspace Cargo.toml changed")
        fi

        # The lock is shared by the whole workspace; a change only
        # matters here if it touches a package this service's dependency
        # closure actually contains.
        if [[ -n "$(changed_files "$deployed_rev" Cargo.lock)" ]] \
            && newer_than_marker Cargo.lock "$marker"; then
            if closure=$(service_closure "$service"); then
                affected=$(comm -12 <(lock_changed_packages "$deployed_rev") \
                                    <(printf '%s\n' "$closure") \
                    | awk '{print $1}' | sort -u || true)
                if [[ -n "$affected" ]]; then
                    reasons+=("dependencies changed: $(echo "$affected" | paste -sd' ')")
                fi
            else
                # Closure unknown — be conservative.
                reasons+=("Cargo.lock changed (dependency closure unresolvable)")
            fi
        fi
    else
        # Old-format marker (no revision): fall back to comparing the
        # deploy time against commit times and any dirty state, with the
        # shared manifests included wholesale.
        if [[ -z "$deployed_ts" ]]; then
            printf 'PENDING  %-20s (no valid deploy timestamp or revision: %q)\n' \
                "$service" "$deployed_raw"
            pending=$((pending + 1))
            pending_services+=("$service")
            continue
        fi

        paths=("${core_paths[@]}")
        [[ -f Cargo.toml ]] && paths+=("Cargo.toml")
        [[ -f Cargo.lock ]] && paths+=("Cargo.lock")

        last_commit_ts=$(git log -1 --format=%ct -- "${paths[@]}" 2>/dev/null || true)
        if [[ -n "$last_commit_ts" && "$last_commit_ts" -gt "$deployed_ts" ]]; then
            reasons+=("new commit $(date -d "@$last_commit_ts" '+%Y-%m-%d %H:%M')")
        fi
        if [[ -n "$(git status --porcelain -- "${paths[@]}" 2>/dev/null)" ]]; then
            reasons+=("uncommitted changes")
        fi
    fi

    if [[ ${#reasons[@]} -gt 0 ]]; then
        reason_str=$(printf '%s; ' "${reasons[@]}")
        printf 'PENDING  %-20s %s — %s\n' \
            "$service" "$deployed_desc" "${reason_str%; }"
        pending=$((pending + 1))
        pending_services+=("$service")
    else
        printf 'current  %-20s %s\n' "$service" "$deployed_desc"
    fi
done <<< "$services"

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
