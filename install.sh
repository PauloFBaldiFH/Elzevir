#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verified bootstrap for the native Elzevir binary. No optional cloud tools installed.
set -euo pipefail
umask 077
version=latest
institution=
identifier=library
login=admin
listen=127.0.0.1:7390
plan=false
firewall=false
download_only=
managed=false
launch=true
repository=https://github.com/PauloFBaldiFH/Elzevir
api=https://api.github.com/repos/PauloFBaldiFH/Elzevir
version_pattern='^v(0|[1-9][0-9]{0,9})\.(0|[1-9][0-9]{0,9})\.(0|[1-9][0-9]{0,9})(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
fail() { printf 'elzevir: %s\n' "$*" >&2; exit 1; }
value() { [[ $# -ge 2 && -n $2 ]] || fail "missing value for $1"; }
valid_version() {
    local prerelease
    [[ ${#1} -le 40 && $1 =~ $version_pattern ]] || return 1
    prerelease=${BASH_REMATCH[4]-}
    [[ ! ${prerelease#-} =~ (^|\.)0[0-9]+(\.|$) ]]
}
add_user_path() {
    local login_profile="$HOME/.profile" profile
    if [[ ":$PATH:" != *":$program_parent:"* ]]; then
        if [[ -r $HOME/.bash_profile ]]; then
            login_profile=$HOME/.bash_profile
        elif [[ -r $HOME/.bash_login ]]; then
            login_profile=$HOME/.bash_login
        fi
        for profile in "$HOME/.bashrc" "$login_profile"; do
            if ! grep -Fxq '# Elzevir PATH' "$profile" 2>/dev/null; then
                cat >> "$profile" <<'PROFILE'

# Elzevir PATH
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) export PATH="$HOME/.local/bin:$PATH" ;;
esac
PROFILE
            fi
        done
    fi
    export PATH="$program_parent:$PATH"
}
while [[ $# -gt 0 ]]; do
    case $1 in
        --version) value "$@"; version=$2; shift 2 ;;
        --name) value "$@"; institution=$2; managed=true; shift 2 ;;
        --id) value "$@"; identifier=$2; managed=true; shift 2 ;;
        --login) value "$@"; login=$2; managed=true; shift 2 ;;
        --listen) value "$@"; listen=$2; managed=true; shift 2 ;;
        --download-only) value "$@"; download_only=$2; shift 2 ;;
        --dry-run) plan=true; shift ;;
        --no-launch) launch=false; shift ;;
        --open-firewall) firewall=true; managed=true; shift ;;
        --help) printf '%s\n' 'Usage: bash install.sh [--version TAG] [--dry-run] [--no-launch] [--download-only NEW_DIRECTORY]' 'Managed service: sudo bash install.sh --name "Library" [--id library] [--login admin] [--listen 127.0.0.1:7390] [--open-firewall]'; exit 0 ;;
        *) fail "unknown option: $1" ;;
    esac
done
[[ $(uname -s) == Linux ]] || fail 'this installer requires Linux'
[[ $(getconf LONG_BIT) == 64 ]] || fail 'a 64-bit operating system is required'
case $(uname -m) in
    x86_64) target=x86_64-unknown-linux-musl ;;
    aarch64|arm64) target=aarch64-unknown-linux-musl ;;
    *) fail 'supported processors: x86_64 and ARM64' ;;
esac
[[ $identifier =~ ^[a-z0-9][a-z0-9-]{0,31}$ && $identifier != *- ]] || fail 'invalid institution identifier'
[[ $login =~ ^[a-zA-Z0-9_.-]{1,64}$ ]] || fail 'invalid administrator login'
[[ $version == latest ]] || valid_version "$version" || fail 'version must be latest or a SemVer tag'
if $managed; then
    binary=/usr/local/bin/elzevir
    [[ $identifier == library ]] || binary=/usr/local/lib/elzevir-$identifier/elzevir
    config=/etc/elzevir/$identifier.toml
    data=/var/lib/elzevir/$identifier
    service=elzevir-$identifier
    [[ $identifier != library ]] || service=elzevir
else
    binary=$HOME/.local/bin/elzevir
    data_home=${XDG_DATA_HOME:-$HOME/.local/share}
    [[ $data_home == /* ]] || data_home=$HOME/.local/share
    data=$data_home/elzevir
    config=
    service=
    libc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || libc=
    if [[ $target == x86_64-unknown-linux-musl && $libc =~ ^glibc\ ([0-9]+)\.([0-9]+)$ ]] &&
        (( BASH_REMATCH[1] > 2 || (BASH_REMATCH[1] == 2 && BASH_REMATCH[2] >= 35) )); then
        target=x86_64-unknown-linux-gnu
    fi
fi
program_parent=${binary%/*}
for utility in curl tar sha256sum awk mktemp stat; do command -v "$utility" >/dev/null || fail "required utility unavailable: $utility"; done
if ! $plan && $managed && [[ -z $download_only ]]; then
    [[ $EUID -eq 0 ]] || fail 'run the installer as root'
    [[ -d /run/systemd/system || -x /sbin/openrc ]] || fail 'systemd or OpenRC is required'
    for utility in install chown chmod id; do command -v "$utility" >/dev/null || fail "required utility unavailable: $utility"; done
    [[ ! -L $binary && ! -L $config && ! -L $data ]] || fail 'installation paths must not be symlinks'
    [[ ! -e $config || -f $config ]] || fail 'configuration must be a regular file'
    [[ ! -e $binary || -f $binary ]] || fail 'binary must be a regular file'
    candidate=$program_parent
    while [[ $candidate != / ]]; do
        [[ ! -L $candidate ]] || fail 'program directories must not be symlinks'
        if [[ -e $candidate ]]; then
            [[ -d $candidate && $(stat -c %u "$candidate") -eq 0 ]] || fail 'program directories must belong to root'
            permissions=$(stat -c %a "$candidate")
            (( (8#$permissions & 022) == 0 )) || fail 'program directories must not be writable by other accounts'
        fi
        candidate=${candidate%/*}
        [[ -n $candidate ]] || candidate=/
    done
    if [[ -e $config ]]; then
        [[ -x $binary && -f $data/elzevir.db ]] || fail 'existing installation is incomplete; inspect it before retrying'
    else
        [[ -n $institution ]] || fail 'set --name for the first installation'
        [[ ! -e $binary ]] || fail 'binary exists without matching configuration; it was preserved'
        [[ ! -e $data ]] || fail 'data directory already exists; configuration will not be guessed'
    fi
fi
request() {
    local url=$1
    shift
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --max-redirs 5 "$@" "$url" ||
        { printf 'elzevir: request failed: %s\n' "$url" >&2; return 1; }
}
api_tag() {
    local endpoint=$1
    local -a headers=()
    command -v python3 >/dev/null || return 1
    if [[ -n ${GITHUB_TOKEN:-} ]]; then
        headers=(--header "Authorization: Bearer $GITHUB_TOKEN")
    fi
    request "$api/$endpoint" "${headers[@]}" --max-time 60 --max-filesize 4194304 --output - |
        python3 -c 'import json, sys
data = sys.stdin.read()
if data:
    releases = json.loads(data)
    if isinstance(releases, dict):
        releases = [releases]
    print(next((r["tag_name"] for r in releases if not r.get("draft")), ""))'
}
resolve_latest() {
    local redirect tag
    if redirect=$(request "$repository/releases/latest" --max-time 60 --output /dev/null --write-out '%{url_effective}'); then
        if [[ $redirect == "$repository/releases/tag/"* ]]; then
            printf '%s\n' "${redirect#"$repository/releases/tag/"}"
            return
        fi
    elif tag=$(api_tag releases/latest) && valid_version "$tag"; then
        printf '%s\n' "$tag"
        return
    fi
    if tag=$(api_tag 'releases?per_page=10') && valid_version "$tag"; then
        printf '%s\n' "$tag"
        return
    fi
    # GitHub publishes this Atom feed even when /latest has no stable tag.
    request "$repository/releases.atom" --max-time 60 --max-filesize 4194304 |
        sed -nE 's@^[[:space:]]*<id>tag:github.com,2008:Repository/[0-9]+/([^<]+)</id>.*@\1@p' |
        sed -n '1p'
}
if [[ $version == latest ]]; then
    version=$(resolve_latest) && valid_version "$version" || fail 'no published release resolved; pass --version with a published tag'
fi
name=elzevir-$version-$target.tar.gz
[[ $target != x86_64-unknown-linux-gnu ]] || name=elzevir-linux-x86_64.tar.gz
base=$repository/releases/download/$version
if $plan; then
    printf 'target=%s\nversion=%s\nbinary=%s\nconfig=%s\ndata=%s\nservice=%s\nfirewall=%s\n' "$target" "$version" "$binary" "$config" "$data" "$service" "$firewall"
    printf 'asset_url=%s\nchecksums_url=%s\n' "$base/$name" "$base/SHA256SUMS"
    exit 0
fi
temporary=$(mktemp -d)
staged_binary=
trap 'if [[ -n $staged_binary ]]; then rm -f -- "$staged_binary"; fi; rm -rf -- "$temporary"' EXIT
fetch() { request "$1" --max-time 900 --max-filesize "${3:-536870912}" --output "$2"; }
fetch "$base/SHA256SUMS" "$temporary/SHA256SUMS" 65536
[[ $(stat -c %s "$temporary/SHA256SUMS") -le 65536 ]] || fail 'checksum document exceeds limit'
expected=$(awk -v name="$name" '$2==name || $2=="*"name { if (found++) exit 2; hash=$1 } END {if(found!=1) exit 1; print hash}' "$temporary/SHA256SUMS") || fail 'package checksum absent or duplicated'
[[ $expected =~ ^[a-fA-F0-9]{64}$ ]] || fail 'invalid package checksum'
fetch "$base/$name" "$temporary/$name"
actual=$(sha256sum "$temporary/$name"); actual=${actual%% *}
[[ ${actual,,} == ${expected,,} ]] || fail 'package checksum mismatch; nothing was installed'
mkdir "$temporary/payload"
entries=$(tar -tzf "$temporary/$name") || fail 'invalid package archive'
while IFS= read -r entry; do case $entry in elzevir|README.md|LICENSE) ;; *) fail 'unexpected package entry' ;; esac; done <<< "$entries"
[[ $(printf '%s\n' "$entries" | awk 'END {print NR}') -eq 3 ]] || fail 'invalid package entry count'
[[ $(tar -tvzf "$temporary/$name" | awk 'substr($0,1,1)!="-" {bad=1} END {print bad+0}') == 0 ]] || fail 'package must contain only regular files'
tar --extract --gzip --no-same-owner --no-same-permissions --directory "$temporary/payload" --file "$temporary/$name" -- elzevir README.md LICENSE
[[ -f $temporary/payload/elzevir && ! -L $temporary/payload/elzevir ]] || fail 'native executable absent'
chmod 700 "$temporary/payload/elzevir"
if [[ -n $download_only ]]; then
    [[ ! -e $download_only && ! -L $download_only ]] || fail 'download destination must be new'
    mkdir -m 700 -- "$download_only"
    cp -- "$temporary/payload/elzevir" "$temporary/payload/README.md" "$temporary/payload/LICENSE" "$download_only/"
    printf 'Verified package saved to %s\n' "$download_only"
    exit 0
fi
if ! $managed; then
    [[ ! -L $binary && ( ! -e $binary || -f $binary ) ]] || fail 'binary destination must be a regular file'
    [[ $("$temporary/payload/elzevir" --version) == "elzevir ${version#v}" ]] || fail 'package version mismatch'
    "$temporary/payload/elzevir" panel --help >/dev/null
    mkdir -p -m 700 -- "$data" "$program_parent"
    staged_binary=$(mktemp "$program_parent/.elzevir-install.XXXXXXXX")
    install -m 755 -- "$temporary/payload/elzevir" "$staged_binary"
    mv -fT -- "$staged_binary" "$binary"
    staged_binary=
    on_path=false
    [[ ":$PATH:" != *":$program_parent:"* ]] || on_path=true
    add_user_path
    elzevir --version
    elzevir panel --help >/dev/null
    printf 'Installed: %s\nData: %s\n' "$binary" "$data"
    command=elzevir
    if ! $on_path; then
        # A piped installer cannot change the PATH of the shell that started it.
        printf '\nThis terminal does not know the elzevir command yet. Run once:\n    source ~/.bashrc\n'
        printf 'or open a new terminal. Until then, use the full path below.\n'
        command=$binary
    fi
    if "$binary" start --help >/dev/null 2>&1; then
        if [[ -n ${DISPLAY:-}${WAYLAND_DISPLAY:-} ]] && "$binary" desktop --help >/dev/null 2>&1; then
            # The launcher is a convenience; a failure must not undo the installation.
            if "$binary" desktop install; then
                printf '\nElzevir is in the application menu and on the desktop.\n'
            else
                printf '\nThe desktop launcher could not be created; use the command below.\n'
            fi
        fi
        printf '\nTo open the panel later:\n    %s\n' "$command"
        # curl | bash leaves stdin on the pipe; the panel reads the terminal itself.
        if $launch && { : </dev/tty; } 2>/dev/null; then
            printf 'Opening the panel now. The first start asks for the library name and the administrator password.\n'
            exec "$binary" start </dev/tty
        fi
        printf 'The first start asks for the library name and the administrator password,\n'
        printf 'then serves the catalog and opens the panel, which shows its address.\n'
    else
        printf '\nThis release has no start command. Next steps:\n'
        printf '    %s init --name "My library" --data-dir %q --config %q\n' "$command" "$data/institution" "$data/elzevir.toml"
        printf '    %s serve --config %q\n' "$command" "$data/elzevir.toml"
    fi
    exit 0
fi
if ! id elzevir >/dev/null 2>&1; then
    if command -v useradd >/dev/null; then useradd --system --user-group --home-dir /var/lib/elzevir --shell /usr/sbin/nologin elzevir;
    elif command -v adduser >/dev/null && command -v addgroup >/dev/null; then addgroup -S elzevir; adduser -S -D -H -G elzevir -h /var/lib/elzevir -s /sbin/nologin elzevir;
    else fail 'a system account creation utility is required'; fi
fi
if [[ -e $config ]]; then
    manager_state=$("$binary" service status --name "$service") || fail 'cannot inspect the existing service'
    if [[ $manager_state == *'"state":"not_installed"'* ]]; then
        chown -R elzevir:elzevir "$data"
        chown root:elzevir /etc/elzevir "$config"
        chmod 640 "$config"
        "$binary" service install --name "$service" --config "$config"
        "$binary" service start --name "$service"
    fi
    "$binary" update --config "$config" --service "$service" --version "$version" --package "$temporary/$name" --checksums "$temporary/SHA256SUMS" --confirm-update --wait
    exit 0
fi
# Init prompts privately on the controlling terminal; no password reaches argv.
install -d -m 750 /etc/elzevir
install -d -m 755 /var/lib/elzevir
case $(stat -f -c %T /var/lib/elzevir) in nfs*|cifs|smb*|9p|fuse.sshfs) fail 'the institution database requires a local filesystem' ;; esac
available=$(stat -f -c '%a %S' /var/lib/elzevir | awk '{printf "%.0f",$1*$2}')
[[ $available -ge 1073741824 ]] || fail 'at least 1 GiB free local space is required'

install -d -m 755 "${binary%/*}"
install -d -m 755 "$program_parent"
install -m 755 "$temporary/payload/elzevir" "$binary"
[[ -r /dev/tty ]] || fail 'initial installation requires a controlling terminal for the private password prompt'
"$binary" init --name "$institution" --config "$config" --data-dir "$data" --login "$login" --listen "$listen" --check-port </dev/tty
chown -R elzevir:elzevir "$data"
chown root:elzevir /etc/elzevir "$config"
chmod 750 /etc/elzevir
chmod 640 "$config"
"$binary" service install --name "$service" --config "$config"
"$binary" service start --name "$service"
healthy=false
for ((attempt=0;attempt<60;attempt++)); do if "$binary" status --config "$config" --json >/dev/null 2>&1; then healthy=true; break; fi; sleep 1; done
$healthy || fail 'service did not become healthy; configuration and data were preserved'
if $firewall; then
    port=${listen##*:}
    [[ $port =~ ^[0-9]+$ && $port -gt 0 && $port -le 65535 ]] || fail 'cannot determine the firewall port'
    if command -v ufw >/dev/null; then ufw allow "$port/tcp";
    elif command -v firewall-cmd >/dev/null; then firewall-cmd --permanent --add-port="$port/tcp"; firewall-cmd --reload;
    else fail 'firewall option requested, but no supported firewall manager is installed'; fi
fi
printf 'Elzevir ready. Open the panel: sudo %s panel --config %s --lang pt-BR\n' "$binary" "$config"
