#!/usr/bin/env bash
# Publish every installed Star Citizen build that the dumps repo does not have yet.
#
# Fired detached from the launcher's post-exit hook (sc-postexit.sh), installed by
# `make hook-install`. The launcher patches the install while you play; once it
# exits, each channel whose build_manifest.id names a build with no tag in
# voidput/sc-gamedata-dumps is extracted and published from a clean `main`
# worktree, never from the dev checkout. Spec: docs/plans/specs/tool-data-sync.md.
#
#   scripts/release-on-exit.sh run        # what the hook runs
#   scripts/release-on-exit.sh check      # installed vs published, per channel
#   scripts/release-on-exit.sh install    # add the hook blocks (SC_AUTO_PUBLISH=1 to publish)
#   scripts/release-on-exit.sh uninstall  # remove them
#
# Environment (defaults in brackets):
#   SC_ROOT              [the Makefile's SC_ROOT]  install with LIVE/ PTU/ inside
#   SC_RELEASE_CHANNELS  [LIVE PTU]
#   SC_AUTO_PUBLISH      [0]  1 publishes; 0 stops after publish-dry and notifies
#   SC_RELEASE_DIR       [~/git/sc-gamedata-release]  the clean worktree
#   SC_DUMPS_REMOTE      [git@github.com:voidput/sc-gamedata-dumps.git]
#   SC_HOOK_DIR          [/mnt/games/star-citizen]  where sc-postexit.sh / sc-prelaunch.sh live
#   SC_RELEASE_DELAY     [60]  seconds to wait before `run` looks, so wine processes settle

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$HERE/.." && pwd)"
SELF="$HERE/$(basename "${BASH_SOURCE[0]}")"

SC_ROOT="${SC_ROOT:-/mnt/games/star-citizen/drive_c/Program Files/Roberts Space Industries/StarCitizen}"
CHANNELS="${SC_RELEASE_CHANNELS:-LIVE PTU}"
AUTO="${SC_AUTO_PUBLISH:-0}"
RELEASE_DIR="${SC_RELEASE_DIR:-$HOME/git/sc-gamedata-release}"
DUMPS_REMOTE="${SC_DUMPS_REMOTE:-git@github.com:voidput/sc-gamedata-dumps.git}"
HOOK_DIR="${SC_HOOK_DIR:-/mnt/games/star-citizen}"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}"
LOCK="$STATE_DIR/sc-gamedata-release.lock"
LOG="$STATE_DIR/sc-gamedata-release.log"

BEGIN="# >>> sc-gamedata hook >>>"
END="# <<< sc-gamedata hook <<<"

log() { printf '%s release-on-exit: %s\n' "$(date '+%F %T')" "$*"; }

notify() {
    log "$*"
    command -v notify-send >/dev/null 2>&1 && notify-send -a sc-gamedata "sc-gamedata release" "$*" || true
}

# Data.Version from a channel's build_manifest.id: the string every dump is
# stamped with and the dumps tag it becomes.
installed_build() {
    local manifest="$SC_ROOT/$1/build_manifest.id"
    [ -f "$manifest" ] || return 1
    jq -er '.Data.RequestedP4kVersion // .Data.Version' "$manifest"
}

# Fingerprint of what the launcher could change under us: the manifest and the
# p4k's size + mtime. Compared before publishing, so a relaunch that patched
# the install mid-extract never ships a torn build.
fingerprint() {
    local dir="$SC_ROOT/$1"
    printf '%s|%s' "$(sha256sum "$dir/build_manifest.id" | cut -d' ' -f1)" "$(stat -c '%s.%Y' "$dir/Data.p4k")"
}

published_tags() {
    git ls-remote --tags --refs "$DUMPS_REMOTE" | sed 's#.*refs/tags/##'
}

game_running() {
    pgrep -f '[S]tarCitizen\.exe|[R]SI Launcher\.exe' >/dev/null 2>&1
}

# A detached worktree at origin/main: the dev checkout's branch, data and
# uncommitted work never reach a publish.
prepare_worktree() {
    git -C "$REPO_DIR" fetch --quiet origin main
    if [ ! -e "$RELEASE_DIR/.git" ]; then
        git -C "$REPO_DIR" worktree add --detach "$RELEASE_DIR" origin/main
    fi
    if [ -n "$(git -C "$RELEASE_DIR" status --porcelain --untracked-files=no)" ]; then
        log "release worktree $RELEASE_DIR has local changes; refusing to use it"
        return 1
    fi
    git -C "$RELEASE_DIR" checkout --quiet --detach origin/main
    make -C "$RELEASE_DIR" build
}

release_channel() {
    local channel="$1" build="$2" before
    local m=(nice -n 19 ionice -c3 make -C "$RELEASE_DIR" "SC_ROOT=$SC_ROOT")
    before="$(fingerprint "$channel")"
    log "$channel $build: extracting"
    "${m[@]}" all "CHANNEL=$channel"
    "${m[@]}" publish-dry "CHANNEL=$channel"
    if [ "$(fingerprint "$channel")" != "$before" ] || game_running; then
        notify "$channel $build: install changed or game started during the extract; not publishing"
        return 1
    fi
    if [ "$AUTO" != 1 ]; then
        notify "$channel $build: dry run done (SC_AUTO_PUBLISH=0). Publish: make -C $RELEASE_DIR publish CHANNEL=$channel"
        return 0
    fi
    "${m[@]}" publish "CHANNEL=$channel"
    notify "$channel $build: published"
}

cmd_run() {
    mkdir -p "$STATE_DIR"
    exec 9>"$LOCK"
    if ! flock -n 9; then
        log "another release is running; skipping"
        return 0
    fi
    sleep "${SC_RELEASE_DELAY:-60}"
    if game_running; then
        log "game or launcher still running; skipping"
        return 0
    fi
    local tags
    if ! tags="$(published_tags)"; then
        log "cannot list $DUMPS_REMOTE tags (offline?); skipping"
        return 0
    fi
    local pending=() channel build
    for channel in $CHANNELS; do
        build="$(installed_build "$channel")" || { log "$channel: not installed"; continue; }
        if grep -qxF "$build" <<<"$tags"; then
            log "$channel $build: already published"
        else
            pending+=("$channel:$build")
        fi
    done
    [ ${#pending[@]} -gt 0 ] || return 0
    prepare_worktree
    local item status=0
    for item in "${pending[@]}"; do
        release_channel "${item%%:*}" "${item#*:}" || { status=1; notify "${item%%:*} ${item#*:}: failed, see $LOG"; }
    done
    return "$status"
}

cmd_check() {
    local tags channel build
    tags="$(published_tags)"
    for channel in $CHANNELS; do
        if ! build="$(installed_build "$channel")"; then
            printf '%-5s not installed\n' "$channel"
        elif grep -qxF "$build" <<<"$tags"; then
            printf '%-5s %s  published\n' "$channel" "$build"
        else
            printf '%-5s %s  NOT published\n' "$channel" "$build"
        fi
    done
}

# Drop any existing block, then put the new one right before the script's
# final `exit 0` (or at the end when there is none).
strip_block() {
    sed -i "\\|^$BEGIN\$|,\\|^$END\$|d" "$1"
}

insert_block() {
    local file="$1" block="$2" tmp
    tmp="$(mktemp)"
    printf '%s\n' "$block" >"$tmp"
    if [ "$(tail -n1 "$file")" = "exit 0" ]; then
        sed -i -e '$d' "$file"
        cat "$tmp" >>"$file"
        echo "exit 0" >>"$file"
    else
        cat "$tmp" >>"$file"
    fi
    rm -f "$tmp"
}

cmd_install() {
    local post="$HOOK_DIR/sc-postexit.sh" pre="$HOOK_DIR/sc-prelaunch.sh"
    [ -f "$post" ] || { echo "no $post"; exit 1; }
    [ "$AUTO" = 1 ] || AUTO=0
    local post_block pre_block
    post_block="$BEGIN
# sc-gamedata: publish any installed LIVE/PTU build the dumps repo lacks.
# Managed by \`make hook-install\` in $REPO_DIR; \`make hook-uninstall\` removes it. Edit outside.
if [ -x \"$SELF\" ]; then
    mkdir -p \"$STATE_DIR\"
    SC_AUTO_PUBLISH=$AUTO setsid nohup \"$SELF\" run >>\"$LOG\" 2>&1 </dev/null &
fi
$END"
    pre_block="$BEGIN
# sc-gamedata: a release still extracting reads Data.p4k; patching it now aborts that publish.
# Managed by \`make hook-install\` in $REPO_DIR; \`make hook-uninstall\` removes it. Edit outside.
if [ -e \"$LOCK\" ] && ! flock -n \"$LOCK\" true 2>/dev/null; then
    echo \"sc-gamedata: release still running ($LOG); it will not publish if the launcher patches\"
    command -v notify-send >/dev/null 2>&1 && notify-send -a sc-gamedata \"sc-gamedata release\" \"Still extracting; a launcher patch now cancels the publish.\"
fi
$END"
    strip_block "$post"; insert_block "$post" "$post_block"
    echo "installed in $post (SC_AUTO_PUBLISH=$AUTO)"
    if [ -f "$pre" ]; then
        strip_block "$pre"; insert_block "$pre" "$pre_block"
        echo "installed in $pre"
    fi
}

cmd_uninstall() {
    local f
    for f in "$HOOK_DIR/sc-postexit.sh" "$HOOK_DIR/sc-prelaunch.sh"; do
        [ -f "$f" ] && grep -qxF "$BEGIN" "$f" && strip_block "$f" && echo "removed from $f"
    done
    return 0
}

case "${1:-}" in
    run) cmd_run ;;
    check) cmd_check ;;
    install) cmd_install ;;
    uninstall) cmd_uninstall ;;
    *) sed -n '2,22p' "$SELF" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
