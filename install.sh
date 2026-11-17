#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verified bootstrap for the native Elzevir binary. No optional cloud tools installed.
set -euo pipefail
umask 077
version=latest
institution=
identifier=library
login=admin
listen=127.0.0.1:8080
plan=false
firewall=false
download_only=
repository=https://github.com/PauloFBaldiFH/Elzevir
fail() { printf 'elzevir: %s\n' "$*" >&2; exit 1; }
value() { [[ $# -ge 2 && -n $2 ]] || fail "missing value for $1"; }
while [[ $# -gt 0 ]]; do
    case $1 in
        --version) value "$@"; version=$2; shift 2 ;;
        --name) value "$@"; institution=$2; shift 2 ;;
        --id) value "$@"; identifier=$2; shift 2 ;;
        --login) value "$@"; login=$2; shift 2 ;;
        --listen) value "$@"; listen=$2; shift 2 ;;
        --download-only) value "$@"; download_only=$2; shift 2 ;;
        --dry-run) plan=true; shift ;;
        --open-firewall) firewall=true; shift ;;
        --help) printf '%s\n' 'Usage: bash install.sh --name "Library" [--version v1.2.3] [--id library] [--login admin] [--listen 127.0.0.1:8080] [--open-firewall] [--dry-run] [--download-only NEW_DIRECTORY]'; exit 0 ;;
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
[[ $version == latest || $version =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail 'version must be latest or a stable SemVer tag'
binary=/usr/local/bin/elzevir
[[ $identifier == library ]] || binary=/usr/local/lib/elzevir-$identifier/elzevir
program_parent=${binary%/*}
config=/etc/elzevir/$identifier.toml
data=/var/lib/elzevir/$identifier
service=elzevir-$identifier
[[ $identifier != library ]] || service=elzevir
if $plan; then
    printf 'target=%s\nversion=%s\nbinary=%s\nconfig=%s\ndata=%s\nservice=%s\nfirewall=%s\n' "$target" "$version" "$binary" "$config" "$data" "$service" "$firewall"
    exit 0
fi
for utility in curl tar sha256sum awk mktemp stat; do command -v "$utility" >/dev/null || fail "required utility unavailable: $utility"; done
if [[ -z $download_only ]]; then
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
if [[ $version == latest ]]; then
    resolved=$(curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --max-redirs 5 --max-time 60 --output /dev/null --write-out '%{url_effective}' "$repository/releases/latest")
    [[ $resolved == "$repository/releases/tag/"* ]] || fail 'cannot resolve a stable official release'
    version=${resolved##*/}
    [[ $version =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail 'latest release is not a stable SemVer tag'
fi
temporary=$(mktemp -d)
trap 'rm -rf -- "$temporary"' EXIT
name=elzevir-$version-$target.tar.gz
base=$repository/releases/download/$version
fetch() { curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --max-redirs 5 --max-time 900 --max-filesize "${3:-536870912}" --output "$2" "$1"; }
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
