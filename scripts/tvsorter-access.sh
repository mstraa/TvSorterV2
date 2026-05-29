#!/usr/bin/env bash
set -Eeuo pipefail

SERVICE_NAME="${TVSORTER_SERVICE_NAME:-tvsorter}"
APP_USER="${TVSORTER_APP_USER:-tvsorter}"
DATA_DIR="${TVSORTER_DATA_DIR:-/var/lib/tvsorter}"
MODE="group"
PATH_TO_CHECK=""

usage() {
  cat <<'USAGE'
Configure TvSorter service access to an existing media mount without changing the mount permissions.

Run inside the TvSorter LXC as root.

Usage:
  tvsorter-access --path /mnt/data/Movies --mode group
  tvsorter-access --path /mnt/data/Movies --mode owner

Modes:
  group   Add the tvsorter service user to a local group with the same numeric GID
          as the target path. Use this when the mount is group-writable.

  owner   Run tvsorter.service as a local user/group matching the numeric owner
          UID/GID of the target path. Use this when only the owner can write.

This script changes only local LXC users, groups, systemd overrides, and TvSorter
state-directory ownership. It does not chown or chmod the media mount.
USAGE
}

log() { printf '[TvSorter Access] %s\n' "$*"; }
die() { printf '[TvSorter Access] ERROR: %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --path) PATH_TO_CHECK="${2:-}"; shift 2;;
    --mode) MODE="${2:-}"; shift 2;;
    -h|--help) usage; exit 0;;
    *) die "Unknown argument: $1";;
  esac
done

[[ "${EUID:-$(id -u)}" -eq 0 ]] || die "Run as root inside the TvSorter LXC"
[[ -n "$PATH_TO_CHECK" ]] || die "Missing --path"
[[ -e "$PATH_TO_CHECK" ]] || die "Path does not exist: $PATH_TO_CHECK"
[[ "$MODE" == "group" || "$MODE" == "owner" ]] || die "--mode must be group or owner"

path_uid="$(stat -c '%u' "$PATH_TO_CHECK")"
path_gid="$(stat -c '%g' "$PATH_TO_CHECK")"
path_mode="$(stat -c '%A' "$PATH_TO_CHECK")"

group_for_gid() {
  local gid="$1" group_name
  group_name="$(getent group "$gid" | cut -d: -f1 || true)"
  if [[ -n "$group_name" ]]; then printf '%s\n' "$group_name"; return 0; fi
  group_name="tvsorter-mount-${gid}"
  groupadd --gid "$gid" "$group_name"
  printf '%s\n' "$group_name"
}

user_for_uid_gid() {
  local uid="$1" gid="$2" user_name
  user_name="$(getent passwd "$uid" | cut -d: -f1 || true)"
  if [[ -n "$user_name" ]]; then printf '%s\n' "$user_name"; return 0; fi
  user_name="tvsorter-mount-${uid}"
  useradd --system --no-create-home --shell /usr/sbin/nologin --uid "$uid" --gid "$gid" "$user_name"
  printf '%s\n' "$user_name"
}

write_override() {
  local user_name="$1" group_name="$2" supplementary_group="$3"
  install -d "/etc/systemd/system/${SERVICE_NAME}.service.d"
  cat >"/etc/systemd/system/${SERVICE_NAME}.service.d/access.conf" <<UNIT
[Service]
User=${user_name}
Group=${group_name}
SupplementaryGroups=${supplementary_group}
UNIT
}

target_group="$(group_for_gid "$path_gid")"

if [[ "$MODE" == "group" ]]; then
  log "Path owner/group/mode: ${path_uid}:${path_gid} ${path_mode} ${PATH_TO_CHECK}"
  log "Adding ${APP_USER} to local group ${target_group} with GID ${path_gid}"
  id "$APP_USER" >/dev/null 2>&1 || die "Local service user does not exist: $APP_USER"
  usermod -aG "$target_group" "$APP_USER"
  install -d "$DATA_DIR"
  chown -R "${APP_USER}:${APP_USER}" "$DATA_DIR"
  write_override "$APP_USER" "$APP_USER" "$target_group"
else
  log "Path owner/group/mode: ${path_uid}:${path_gid} ${path_mode} ${PATH_TO_CHECK}"
  target_user="$(user_for_uid_gid "$path_uid" "$path_gid" "$target_group")"
  log "Configuring ${SERVICE_NAME}.service to run as ${target_user}:${target_group} (${path_uid}:${path_gid})"
  install -d "$DATA_DIR"
  chown -R "${target_user}:${target_group}" "$DATA_DIR"
  write_override "$target_user" "$target_group" "$target_group"
fi

systemctl daemon-reload
systemctl restart "${SERVICE_NAME}.service"

if systemctl is-active --quiet "${SERVICE_NAME}.service"; then
  log "${SERVICE_NAME}.service restarted"
else
  systemctl --no-pager --full status "${SERVICE_NAME}.service" || true
  die "${SERVICE_NAME}.service did not start cleanly"
fi

service_user="$(systemctl show "${SERVICE_NAME}.service" -P User)"
if runuser -u "$service_user" -- test -w "$PATH_TO_CHECK"; then
  log "Write check passed for ${PATH_TO_CHECK}"
else
  die "Write check still failed. Try --mode owner if --mode group was used, or check parent directory ACLs/mount options."
fi
