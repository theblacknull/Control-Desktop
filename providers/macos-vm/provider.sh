#!/bin/sh
set -eu

json_str() {
  printf '%s' "$1" | awk '
    BEGIN { RS = "\001" }
    {
      gsub(/\\/, "\\\\")
      gsub(/"/, "\\\"")
      gsub(/\t/, "\\t")
      gsub(/\r/, "\\r")
      gsub(/\n/, "\\n")
      gsub(/[[:cntrl:]]/, "")
      printf "%s", $0
    }
  '
}

emit_ok() {
  printf '{"ok":true,"state":"%s","message":"%s"' "$1" "$(json_str "$2")"
  if [ -n "${3:-}" ]; then
    printf ',"next":"%s"' "$(json_str "$3")"
  fi
  printf '}\n'
}

emit_fail() {
  printf '{"ok":false,"code":"%s","message":"%s","suggestion":"%s"}\n' \
    "$1" "$(json_str "$2")" "$(json_str "$3")"
}

AD_SANDBOX_DIR=${AD_SANDBOX_DIR:-}
AD_PROVIDER_DIR=${AD_PROVIDER_DIR:-}
AD_BINARY=${AD_BINARY:-}
VMHOST=${VMHOST:-$AD_PROVIDER_DIR/build/vmhost}
SCRIPT_PATH=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
verb=${1:-}
if [ "$#" -gt 0 ]; then shift; fi

require_provider_environment() {
  if [ -z "$AD_PROVIDER_DIR" ]; then
    emit_fail INVALID_ARGS "agent-desktop did not pass the provider directory this script needs." "Run an agent-desktop install or sandbox command instead of calling the provider directly."
    exit 1
  fi
  BUILD_DIR=$AD_PROVIDER_DIR/build
  BUILD_LOG=$BUILD_DIR/build.log
}

require_sandbox_environment() {
  require_provider_environment
  if [ -z "$AD_SANDBOX_DIR" ]; then
    emit_fail INVALID_ARGS "agent-desktop did not pass the sandbox directory this script needs." "Run an agent-desktop sandbox command instead of calling the provider directly."
    exit 1
  fi
  VM_DIR=$AD_SANDBOX_DIR/vm
}

valid_name() {
  case $1 in
    ''|.|..|*/*) return 1 ;;
    *) return 0 ;;
  esac
}

valid_guest_user() {
  case $1 in
    ''|-*|*[!A-Za-z0-9._-]*) return 1 ;;
    *) return 0 ;;
  esac
}

host_supported() {
  architecture=$(uname -m 2>/dev/null || true)
  if [ "$architecture" != arm64 ]; then
    emit_ok unavailable "macOS VMs need an Apple Silicon Mac." "Use an Apple Silicon Mac, then run agent-desktop install macos-vm."
    return 1
  fi
  if [ "$(sysctl -n kern.hv_support 2>/dev/null || true)" != 1 ]; then
    emit_ok unavailable "This Mac cannot run virtual machines (it may itself be a virtual machine)."
    return 1
  fi
  if ! xcrun --find swiftc >/dev/null 2>&1; then
    emit_ok unavailable "Xcode Command Line Tools are not installed." "xcode-select --install"
    return 1
  fi
  version=$(sw_vers -productVersion 2>/dev/null || true)
  major=${version%%.*}
  case $major in ''|*[!0-9]*) major=0 ;; esac
  if [ "$major" -lt 13 ]; then
    emit_ok unavailable "macOS VMs need macOS 13 or later on the host." "Upgrade this Mac to macOS 13 or later, then run agent-desktop install macos-vm."
    return 1
  fi
  return 0
}

provider_ready() {
  [ -f "$BUILD_DIR/vmhost" ] || return 1
  if ! entitlements=$(codesign -d --entitlements - "$BUILD_DIR/vmhost" 2>&1); then return 1; fi
  case $entitlements in
    *com.apple.security.virtualization*) return 0 ;;
    *) return 1 ;;
  esac
}

do_check() {
  if ! host_supported; then return 0; fi
  if provider_ready; then
    emit_ok ready "The macOS VM provider is ready."
  else
    emit_ok missing "The macOS VM host has not been built and signed." "agent-desktop install macos-vm"
  fi
}

do_install() {
  if provider_ready; then
    emit_ok ready "The macOS VM provider is ready."
    return 0
  fi
  if ! host_supported; then return 0; fi
  if ! mkdir -p "$BUILD_DIR" || ! : > "$BUILD_LOG"; then
    emit_fail ACTION_FAILED "The macOS VM build directory is not writable." "Make $AD_PROVIDER_DIR writable, then run agent-desktop install macos-vm."
    return 1
  fi
  if [ ! -f "$AD_PROVIDER_DIR/vmhost.swift" ] || [ ! -f "$AD_PROVIDER_DIR/vm.entitlements" ]; then
    printf '%s\n' "The VM host source or entitlements file is missing from $AD_PROVIDER_DIR." >> "$BUILD_LOG"
    emit_fail ACTION_FAILED "The macOS VM host could not be installed; see $BUILD_LOG." "Read $BUILD_LOG, then run agent-desktop install macos-vm again."
    return 1
  fi
  if ! xcrun swiftc -O "$AD_PROVIDER_DIR/vmhost.swift" -o "$BUILD_DIR/vmhost" >> "$BUILD_LOG" 2>&1; then
    rm -f "$BUILD_DIR/vmhost"
    emit_fail ACTION_FAILED "The macOS VM host could not be compiled; see $BUILD_LOG." "Read $BUILD_LOG, then run agent-desktop install macos-vm again."
    return 1
  fi
  if ! codesign -f -s - --entitlements "$AD_PROVIDER_DIR/vm.entitlements" "$BUILD_DIR/vmhost" >> "$BUILD_LOG" 2>&1; then
    rm -f "$BUILD_DIR/vmhost"
    emit_fail ACTION_FAILED "The macOS VM host could not be signed; see $BUILD_LOG." "Read $BUILD_LOG, then run agent-desktop install macos-vm again."
    return 1
  fi
  if ! provider_ready; then
    rm -f "$BUILD_DIR/vmhost"
    printf '%s\n' 'The signed VM host does not contain the virtualization entitlement.' >> "$BUILD_LOG"
    emit_fail ACTION_FAILED "The macOS VM host is missing its virtualization entitlement; see $BUILD_LOG." "Read $BUILD_LOG, then run agent-desktop install macos-vm again."
    return 1
  fi
  emit_ok ready "The macOS VM host was built and signed; the provider is ready."
}

vm_bundle_complete() {
  [ -f "$1/Disk.img" ] && [ -f "$1/AuxiliaryStorage" ] && \
    [ -f "$1/HardwareModel" ] && [ -f "$1/MachineIdentifier" ] && [ -f "$1/config.json" ] && \
    [ -f "$1/share/guest-setup.sh" ] && [ -f "$1/share/id_ed25519.pub" ] && \
    [ -f "$1/share/agent-desktop" ]
}

target_has_sandbox_data() {
  [ -e "$VM_DIR" ] || [ -e "$AD_SANDBOX_DIR/id_ed25519" ] || \
    [ -e "$AD_SANDBOX_DIR/id_ed25519.pub" ] || [ -e "$AD_SANDBOX_DIR/known_hosts" ] || \
    [ -e "$AD_SANDBOX_DIR/guest_user" ] || [ -e "$AD_SANDBOX_DIR/setup_done" ]
}

vm_process_state() {
  state_file=$1
  if [ ! -f "$state_file" ]; then printf 'absent\n'; return 0; fi
  state_pid=$(cat "$state_file" 2>/dev/null || true)
  case $state_pid in ''|*[!0-9]*) printf 'stale\n'; return 0 ;; esac
  case $state_pid in *[1-9]*) ;; *) printf 'stale\n'; return 0 ;; esac
  if ! kill -0 "$state_pid" 2>/dev/null; then printf 'dead\n'; return 0; fi
  state_comm=$(ps -o comm= -p "$state_pid" 2>/dev/null | tr -d '\r' || true)
  case $state_comm in *vmhost) ;; *) printf 'stale\n'; return 0 ;; esac
  state_command=$(ps -ww -o command= -p "$state_pid" 2>/dev/null | tr -d '\r' | sed -e 's/[[:space:]]*$//' || true)
  case $state_command in
    *" run $VM_DIR"|*" run $VM_DIR --window") printf 'live\n' ;;
    *) printf 'stale\n' ;;
  esac
}

job_is_running() {
  [ -f "$VM_DIR/install.pid" ] || return 1
  JOB_PID=$(cat "$VM_DIR/install.pid" 2>/dev/null || true)
  case $JOB_PID in ''|*[!0-9]*) return 2 ;; esac
  case $JOB_PID in *[1-9]*) ;; *) return 2 ;; esac
  kill -0 "$JOB_PID" 2>/dev/null || return 2
  JOB_COMMAND=$(ps -ww -o command= -p "$JOB_PID" 2>/dev/null || true)
  case $JOB_COMMAND in *"$SCRIPT_PATH __install_job "*|*"$SCRIPT_PATH __install_job") return 0 ;; *) return 2 ;; esac
}

broken_state() {
  emit_ok broken "$1" "agent-desktop sandbox rm $2"
}

last_progress() {
  [ -f "$VM_DIR/install.log" ] || return 0
  grep -Eo '[0-9]+%' "$VM_DIR/install.log" 2>/dev/null | tail -n 1 || true
}

gigabytes() {
  awk -v bytes="$1" 'BEGIN { printf "%.1f", bytes / 1000000000 }'
}

download_message() {
  download_path=$(cat "$VM_DIR/download.path" 2>/dev/null || true)
  download_done=$(stat -f %z "$download_path" 2>/dev/null || printf '0\n')
  case $download_done in ''|*[!0-9]*) download_done=0 ;; esac
  download_total=$(cat "$VM_DIR/download.size" 2>/dev/null || true)
  case $download_total in
    ''|*[!0-9]*|0)
      printf 'Sandbox %s is downloading the macOS image: %s GB so far.\n' \
        "$1" "$(gigabytes "$download_done")"
      ;;
    *)
      download_percent=$(awk -v done="$download_done" -v total="$download_total" \
        'BEGIN { printf "%d", done * 100 / total }')
      printf 'Sandbox %s is downloading the macOS image: %s of %s GB (%s%%).\n' \
        "$1" "$(gigabytes "$download_done")" "$(gigabytes "$download_total")" "$download_percent"
      ;;
  esac
}

catalog_firmware() {
  tr '<' '\n' < "$1" | awk '
    function newer(candidate, current, candidate_parts, current_parts, candidate_count, current_count, i) {
      if (current == "") return 1
      candidate_count = split(candidate, candidate_parts, /[.]/)
      current_count = split(current, current_parts, /[.]/)
      for (i = 1; i <= candidate_count || i <= current_count; i++) {
        if (candidate_parts[i] + 0 > current_parts[i] + 0) return 1
        if (candidate_parts[i] + 0 < current_parts[i] + 0) return 0
      }
      return 0
    }
    /^key>Restore$/ { awaiting_restore = 1; next }
    awaiting_restore && /^dict>$/ { in_restore = 1; awaiting_restore = 0; next }
    in_restore && /^key>(FirmwareURL|FirmwareSHA1|ProductVersion)$/ {
      field = substr($0, 5)
      next
    }
    in_restore && /^string>/ {
      value = substr($0, 8)
      if (field == "FirmwareURL") url = value
      if (field == "FirmwareSHA1") sha = value
      if (field == "ProductVersion") version = value
      field = ""
      next
    }
    in_restore && /^\/dict>/ {
      if (url != "" && sha != "" && version ~ /^[0-9]+([.][0-9]+)*$/ && newer(version, best_version)) {
        best_url = url
        gsub(/&amp;/, "\\&", best_url)
        best_sha = sha
        best_version = version
      }
      in_restore = 0
      url = ""
      sha = ""
      version = ""
      field = ""
    }
    END {
      if (best_url != "" && best_sha != "") {
        print best_url
        print best_sha
      }
    }
  '
}

copy_guest_share() {
  share=$1/share
  mkdir -p "$share" || return 1
  cp "$AD_PROVIDER_DIR/guest-setup.sh" "$share/guest-setup.sh" || return 1
  cp "$AD_SANDBOX_DIR/id_ed25519.pub" "$share/id_ed25519.pub" || return 1
  cp "$AD_BINARY" "$share/agent-desktop" || return 1
  chmod 0755 "$share/guest-setup.sh" "$share/agent-desktop" || return 1
}

create_guest_identity() {
  if [ ! -f "$AD_BINARY" ]; then
    emit_fail ACTION_FAILED "The host agent-desktop binary is missing." "Run agent-desktop sandbox create $1 again after rebuilding agent-desktop."
    return 1
  fi
  guest_user=${AD_SET_USER:-}
  if [ -z "$guest_user" ]; then guest_user=$(id -un 2>/dev/null || true); fi
  if [ -z "$guest_user" ]; then
    emit_fail ACTION_FAILED "The guest user name could not be determined." "Run agent-desktop sandbox create $1 --set user=<name>."
    return 1
  fi
  if ! valid_guest_user "$guest_user"; then
    emit_fail INVALID_ARGS "The guest user name contains unsupported characters." "Run agent-desktop sandbox create $1 --set user=<short-name>."
    return 1
  fi
  if ! ssh-keygen -t ed25519 -N '' -f "$AD_SANDBOX_DIR/id_ed25519" >/dev/null 2>&1; then
    emit_fail ACTION_FAILED "The SSH key for sandbox $1 could not be created." "Run agent-desktop sandbox create $1 again."
    return 1
  fi
  if ! chmod 0600 "$AD_SANDBOX_DIR/id_ed25519" || \
    ! : > "$AD_SANDBOX_DIR/known_hosts" || \
    ! printf '%s\n' "$guest_user" > "$AD_SANDBOX_DIR/guest_user"; then
    emit_fail ACTION_FAILED "The SSH settings for sandbox $1 could not be saved securely." "Run agent-desktop sandbox create $1 again after checking directory permissions."
    return 1
  fi
}

source_is_eligible() {
  [ -f "$1/setup_done" ] || return 1
  [ ! -e "$1/vm/vmhost.pid" ] || return 1
  [ -f "$1/id_ed25519" ] && [ -f "$1/id_ed25519.pub" ] && \
    [ -f "$1/known_hosts" ] && [ -f "$1/guest_user" ] || return 1
  source_user=$(cat "$1/guest_user" 2>/dev/null || true)
  valid_guest_user "$source_user" || return 1
  vm_bundle_complete "$1/vm"
}

file_mtime() {
  stat -f %m "$1" 2>/dev/null || stat -c %Y "$1" 2>/dev/null || printf '0\n'
}

newest_source() {
  parent=$(dirname "$AD_SANDBOX_DIR")
  newest=
  newest_time=0
  for candidate in "$parent"/*; do
    [ -d "$candidate" ] || continue
    [ "$candidate" != "$AD_SANDBOX_DIR" ] || continue
    source_is_eligible "$candidate" || continue
    candidate_time=$(file_mtime "$candidate/setup_done")
    case $candidate_time in ''|*[!0-9]*) candidate_time=0 ;; esac
    if [ "$candidate_time" -ge "$newest_time" ]; then
      newest=$candidate
      newest_time=$candidate_time
    fi
  done
  printf '%s\n' "$newest"
}

do_install_job() {
  mode=$1
  ipsw=$2
  cache_dir=$3
  guest_user=$4
  mkdir -p "$VM_DIR/share"
  if [ "$mode" = download ]; then
    mkdir -p "$cache_dir"
    catalog=$cache_dir/catalog.xml
    if ! curl -fsSL -o "$catalog" 'https://mesu.apple.com/assets/macos/com_apple_macOSIPSW/com_apple_macOSIPSW.xml'; then
      printf '%s\n' 'The Apple macOS image catalog could not be downloaded.'
      return 1
    fi
    firmware=$(catalog_firmware "$catalog")
    url=$(printf '%s\n' "$firmware" | sed -n '1p')
    expected=$(printf '%s\n' "$firmware" | sed -n '2p' | tr 'A-F' 'a-f')
    case $url in https://*|http://*) ;; *) printf '%s\n' 'The Apple macOS image catalog has no firmware URL and SHA-1 pair.'; return 1 ;; esac
    case $expected in ''|*[!0-9a-f]*) printf '%s\n' 'The Apple macOS image catalog has no firmware URL and SHA-1 pair.'; return 1 ;; esac
    if [ "${#expected}" -ne 40 ]; then
      printf '%s\n' 'The Apple macOS image catalog has no firmware URL and SHA-1 pair.'
      return 1
    fi
    image_name=${url##*/}
    image_name=${image_name%%\?*}
    [ -n "$image_name" ] || image_name=latest.ipsw
    ipsw=$cache_dir/$image_name
    image_size=$(curl -fsSIL "$url" 2>/dev/null | tr -d '\r' | \
      awk 'tolower($1) == "content-length:" { size = $2 } END { if (size != "") print size }')
    case $image_size in
      ''|*[!0-9]*) ;;
      *) printf '%s\n' "$image_size" > "$VM_DIR/download.size" ;;
    esac
    printf '%s\n' "$ipsw" > "$VM_DIR/download.path"
    if ! curl -C - -fsSL -o "$ipsw" "$url"; then
      printf '%s\n' 'The latest Apple macOS image could not be downloaded.'
      return 1
    fi
    actual=$(shasum -a 1 "$ipsw" 2>/dev/null | awk '{ print tolower($1) }' || true)
    if [ -z "$actual" ] || [ "$expected" != "$actual" ]; then
      rm -f "$ipsw"
      printf '%s\n' 'The downloaded Apple macOS image did not match its published SHA-1.'
      return 1
    fi
    rm -f "$VM_DIR/download.path" "$VM_DIR/download.size"
  fi
  if ! "$VMHOST" install "$VM_DIR" "$ipsw"; then return 1; fi
  if ! copy_guest_share "$VM_DIR"; then
    printf '%s\n' 'The guest setup files could not be copied into the shared folder.'
    return 1
  fi
  printf '%s\n' "$guest_user" > "$AD_SANDBOX_DIR/guest_user"
  : > "$VM_DIR/install.done"
}

start_install_job() {
  name=$1
  mode=$2
  ipsw=$3
  guest_user=$4
  cache_dir=$(dirname "$AD_SANDBOX_DIR")/../cache/macos-vm
  if ! mkdir -p "$VM_DIR" || ! : > "$VM_DIR/install.log"; then
    emit_fail ACTION_FAILED "The installation log for sandbox $name could not be created." "Run agent-desktop sandbox status $name."
    return 1
  fi
  if ! command -v nohup >/dev/null 2>&1; then
    emit_fail ACTION_FAILED "The host cannot detach the macOS installation job." "Install the standard macOS command-line tools, then run agent-desktop sandbox create $name again."
    return 1
  fi
  nohup sh "$SCRIPT_PATH" __install_job "$mode" "$ipsw" "$cache_dir" "$guest_user" \
    >> "$VM_DIR/install.log" 2>&1 < /dev/null &
  install_job_pid=$!
  if ! printf '%s\n' "$install_job_pid" > "$VM_DIR/install.pid"; then
    kill -TERM "$install_job_pid" 2>/dev/null || true
    emit_fail ACTION_FAILED "The installation process for sandbox $name could not be recorded." "Run agent-desktop sandbox status $name."
    return 1
  fi
  emit_ok working "Sandbox $name is installing macOS." "agent-desktop sandbox status $name"
}

do_create() {
  name=${1:-}
  if ! valid_name "$name"; then
    emit_fail INVALID_ARGS "The create verb needs a valid sandbox name." "Run agent-desktop sandbox create <name> --provider macos-vm."
    return 1
  fi
  if target_has_sandbox_data; then
    emit_fail ACTION_FAILED "Sandbox $name already has provider data." "Run agent-desktop sandbox status $name."
    return 1
  fi
  if ! host_supported; then return 0; fi
  if ! provider_ready; then
    emit_fail ACTION_FAILED "The macOS VM host is not installed." "Run agent-desktop install macos-vm."
    return 1
  fi
  if [ -n "${AD_SET_IPSW:-}" ] && [ ! -f "$AD_SET_IPSW" ]; then
    emit_fail ACTION_FAILED "The supplied macOS image file was not found." "Run agent-desktop sandbox create $name --set ipsw=<path> with an existing image file."
    return 1
  fi
  source=
  if [ -n "${AD_SET_FROM:-}" ] && [ -z "${AD_SET_IPSW:-}" ]; then
    if ! valid_name "$AD_SET_FROM"; then
      emit_fail INVALID_ARGS "The source sandbox name is invalid." "Run agent-desktop sandbox create $name --set from=<sibling-name>."
      return 1
    fi
    source=$(dirname "$AD_SANDBOX_DIR")/$AD_SET_FROM
    if [ ! -d "$source" ]; then
      emit_fail SANDBOX_NOT_FOUND "No sandbox named $AD_SET_FROM exists." "Run \`agent-desktop sandbox list\` to see the sandboxes that exist."
      return 1
    fi
    if [ ! -f "$source/setup_done" ] || ! vm_bundle_complete "$source/vm"; then
      emit_fail ACTION_FAILED "Sandbox $AD_SET_FROM is not a completed setup source." "Run agent-desktop sandbox status $AD_SET_FROM."
      return 1
    fi
    if [ -f "$source/vm/vmhost.pid" ]; then
      source_state=$(vm_process_state "$source/vm/vmhost.pid")
      if [ "$source_state" = live ]; then
        emit_fail ACTION_FAILED "Stop $AD_SET_FROM first." "Run agent-desktop sandbox stop $AD_SET_FROM."
        return 1
      fi
      if [ "$source_state" = stale ]; then
        emit_fail ACTION_FAILED "Sandbox $AD_SET_FROM has a stale or foreign VM process record." "Run agent-desktop sandbox status $AD_SET_FROM."
        return 1
      fi
      if [ "$source_state" = dead ]; then
        rm -f "$source/vm/vmhost.pid"
      fi
    fi
    source_is_eligible "$source" || {
      emit_fail ACTION_FAILED "Sandbox $AD_SET_FROM is not stopped and ready to clone." "Run agent-desktop sandbox status $AD_SET_FROM."
      return 1
    }
  elif [ -z "${AD_SET_IPSW:-}" ]; then
    source=$(newest_source)
  fi
  source_name=${source##*/}
  if [ -n "$source" ]; then
    if ! cp "$source/id_ed25519" "$AD_SANDBOX_DIR/id_ed25519" || \
      ! cp "$source/id_ed25519.pub" "$AD_SANDBOX_DIR/id_ed25519.pub" || \
      ! cp "$source/known_hosts" "$AD_SANDBOX_DIR/known_hosts" || \
      ! cp "$source/guest_user" "$AD_SANDBOX_DIR/guest_user"; then
      emit_fail ACTION_FAILED "The SSH settings could not be copied from sandbox $source_name." "Run agent-desktop sandbox status $source_name."
      return 1
    fi
    if ! chmod 0600 "$AD_SANDBOX_DIR/id_ed25519"; then
      emit_fail ACTION_FAILED "The SSH key for sandbox $name could not be protected." "Run agent-desktop sandbox create $name again after checking directory permissions."
      return 1
    fi
    if ! "$VMHOST" clone "$source/vm" "$VM_DIR" >/dev/null 2>&1; then
      emit_fail ACTION_FAILED "The VM for sandbox $name could not be cloned." "Run agent-desktop sandbox status $source_name."
      return 1
    fi
    if ! copy_guest_share "$VM_DIR"; then
      emit_fail ACTION_FAILED "The guest setup files could not be copied into sandbox $name." "Run agent-desktop sandbox status $name."
      return 1
    fi
    if ! cp "$source/setup_done" "$AD_SANDBOX_DIR/setup_done"; then
      emit_fail ACTION_FAILED "The setup record could not be copied from sandbox $source_name." "Run agent-desktop sandbox status $source_name."
      return 1
    fi
    emit_ok stopped "Sandbox $name is a stopped copy of sandbox $source_name." "agent-desktop sandbox start $name"
    return 0
  fi
  if ! create_guest_identity "$name"; then return 1; fi
  if [ -n "${AD_SET_IPSW:-}" ]; then
    start_install_job "$name" supplied "$AD_SET_IPSW" "$(cat "$AD_SANDBOX_DIR/guest_user")"
  else
    start_install_job "$name" download '' "$(cat "$AD_SANDBOX_DIR/guest_user")"
  fi
}

guest_user() {
  [ -s "$AD_SANDBOX_DIR/guest_user" ] || return 1
  stored_guest_user=$(cat "$AD_SANDBOX_DIR/guest_user" 2>/dev/null || true)
  valid_guest_user "$stored_guest_user" || return 1
  printf '%s\n' "$stored_guest_user"
}

guest_transport() {
  transport_tool=$1
  transport_budget=$2
  shift 2
  if [ "$transport_budget" = probe ]; then
    set -- -o ServerAliveInterval=3 -o ServerAliveCountMax=3 "$@"
  fi
  "$transport_tool" -i "$AD_SANDBOX_DIR/id_ed25519" \
    -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=accept-new \
    -o "UserKnownHostsFile=$AD_SANDBOX_DIR/known_hosts" \
    -o BatchMode=yes \
    -o ConnectTimeout=5 \
    -o ConnectionAttempts=1 \
    "$@"
}

needs_setup() {
  emit_ok needs_setup "$1" "agent-desktop sandbox status $2"
}

setup_window() {
  needs_setup "Sandbox $1 is running in a setup window; enable Remote Login, run guest-setup.sh in Terminal, and grant Accessibility permission." "$1"
}

guest_not_answering() {
  if [ -f "$AD_SANDBOX_DIR/setup_done" ]; then
    emit_ok working "Sandbox $1 is running and the guest is still starting." "agent-desktop sandbox status $1"
  else
    needs_setup "Remote Login is unreachable, agent-desktop is not installed, and Accessibility is not granted in the guest." "$1"
  fi
}

missing_guest_binary() {
  next="agent-desktop sandbox status $1"
  [ ! -f "$AD_SANDBOX_DIR/setup_done" ] || next="agent-desktop sandbox exec $1 -- version"
  emit_ok needs_setup "agent-desktop is not installed in the guest, so Accessibility permission cannot yet be checked." "$next"
}

check_guest_setup() {
  name=$1
  if ! user=$(guest_user); then
    needs_setup "Sandbox $name needs a guest user before SSH setup can continue." "$name"
    return 0
  fi
  ip_json=$("$VMHOST" ip "$VM_DIR" 2>/dev/null || true)
  ip=$(printf '%s' "$ip_json" | sed -n 's/.*"ip"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
  if [ -z "$ip" ]; then
    guest_not_answering "$name"
    return 0
  fi
  target=$user@$ip
  probe_status=0
  permissions=$(guest_transport ssh probe "$target" 'if [ ! -x ~/.local/bin/agent-desktop ]; then exit 3; fi; ~/.local/bin/agent-desktop permissions' 2>/dev/null) || probe_status=$?
  if [ "$probe_status" -eq 255 ]; then
    guest_not_answering "$name"
    return 0
  fi
  if [ "$probe_status" -eq 3 ]; then
    missing_guest_binary "$name"
    return 0
  fi
  compact=$(printf '%s' "$permissions" | tr -d '[:space:]')
  case $compact in
    *'"accessibility":{"state":"granted"'*)
      : > "$AD_SANDBOX_DIR/setup_done"
      emit_ok running "Sandbox $name is running and guest setup is complete."
      ;;
    *) needs_setup "Accessibility permission is not granted for agent-desktop in the guest." "$name" ;;
  esac
}

installation_state() {
  if [ -f "$VM_DIR/install.done" ]; then return 1; fi
  if job_is_running; then return 0; fi
  if [ -f "$VM_DIR/install.pid" ]; then return 2; fi
  return 1
}

installation_gate() {
  install_status=0
  installation_state || install_status=$?
  if [ "$install_status" -eq 0 ]; then
    if [ -f "$VM_DIR/download.path" ]; then
      emit_ok working "$(download_message "$1")" "agent-desktop sandbox status $1"
      return 0
    fi
    progress=$(last_progress)
    [ -n "$progress" ] || progress='not available'
    emit_ok working "Sandbox $1 is installing macOS; the latest progress is $progress." "agent-desktop sandbox status $1"
    return 0
  fi
  if [ "$install_status" -eq 2 ] || ! vm_bundle_complete "$VM_DIR"; then
    broken_state "Sandbox $1 has incomplete or failed VM installation files." "$1"
    return 0
  fi
  return 1
}

launch_vmhost() {
  mode=$1
  if [ ! -x "$VMHOST" ] || ! command -v nohup >/dev/null 2>&1 || \
    ! : >> "$VM_DIR/vmhost.log"; then
    return 1
  fi
  if [ "$mode" = window ]; then
    nohup "$VMHOST" run "$VM_DIR" --window >> "$VM_DIR/vmhost.log" 2>&1 < /dev/null &
  else
    nohup "$VMHOST" run "$VM_DIR" >> "$VM_DIR/vmhost.log" 2>&1 < /dev/null &
  fi
  launched_pid=$!
  attempts=0
  while [ "$attempts" -lt 10 ]; do
    if [ "$(vm_process_state "$VM_DIR/vmhost.pid")" = live ]; then return 0; fi
    kill -0 "$launched_pid" 2>/dev/null || return 1
    sleep 0.1
    attempts=$((attempts + 1))
  done
  kill -TERM "$launched_pid" 2>/dev/null || true
  return 1
}

do_start() {
  name=${1:-}
  if ! valid_name "$name"; then
    emit_fail INVALID_ARGS "The start verb needs a valid sandbox name." "Run agent-desktop sandbox start <name>."
    return 1
  fi
  if installation_gate "$name"; then return 0; fi
  vm_state=$(vm_process_state "$VM_DIR/vmhost.pid")
  if [ "$vm_state" = live ]; then
    if [ -f "$AD_SANDBOX_DIR/setup_done" ]; then
      emit_ok running "Sandbox $name is already running."
    else
      setup_window "$name"
    fi
    return 0
  fi
  if [ "$vm_state" = dead ]; then
    rm -f "$VM_DIR/vmhost.pid"
  fi
  if [ "$vm_state" = stale ]; then
    broken_state "Sandbox $name has a dead or foreign VM process record." "$name"
    return 0
  fi
  if ! host_supported; then return 0; fi
  if ! provider_ready; then
    emit_fail ACTION_FAILED "The macOS VM host is not installed." "Run agent-desktop install macos-vm."
    return 1
  fi
  if [ -f "$AD_SANDBOX_DIR/setup_done" ]; then
    if ! launch_vmhost headless; then
      emit_fail ACTION_FAILED "The VM host for sandbox $name did not start; see $VM_DIR/vmhost.log." "Run agent-desktop sandbox status $name."
      return 1
    fi
    emit_ok running "Sandbox $name is running headlessly."
  else
    if ! launch_vmhost window; then
      emit_fail ACTION_FAILED "The VM host for sandbox $name did not start; see $VM_DIR/vmhost.log." "Run agent-desktop sandbox status $name."
      return 1
    fi
    setup_window "$name"
  fi
}

do_stop() {
  name=${1:-}
  if ! valid_name "$name"; then
    emit_fail INVALID_ARGS "The stop verb needs a valid sandbox name." "Run agent-desktop sandbox stop <name>."
    return 1
  fi
  vm_state=$(vm_process_state "$VM_DIR/vmhost.pid")
  if [ "$vm_state" = dead ]; then
    rm -f "$VM_DIR/vmhost.pid"
    emit_ok stopped "Sandbox $name is stopped; its VM did not shut down cleanly." "agent-desktop sandbox start $name"
    return 0
  fi
  if [ "$vm_state" = absent ]; then
    emit_ok stopped "Sandbox $name is already stopped."
    return 0
  fi
  if [ "$vm_state" = stale ]; then
    broken_state "Sandbox $name has a dead or foreign VM process record that was not signalled." "$name"
    return 0
  fi
  stop_pid=$(cat "$VM_DIR/vmhost.pid" 2>/dev/null || true)
  if ! kill -TERM "$stop_pid" 2>/dev/null; then
    broken_state "Sandbox $name could not signal its verified VM host process." "$name"
    return 0
  fi
  attempts=0
  while kill -0 "$stop_pid" 2>/dev/null; do
    if [ "$attempts" -ge 150 ]; then
      emit_fail TIMEOUT "Sandbox $name did not stop within 30 seconds." "Run agent-desktop sandbox status $name."
      return 1
    fi
    sleep 0.2
    attempts=$((attempts + 1))
  done
  emit_ok stopped "Sandbox $name is stopped."
}

do_status() {
  name=${1:-}
  if ! valid_name "$name"; then
    emit_fail INVALID_ARGS "The status verb needs a valid sandbox name." "Run agent-desktop sandbox status <name>."
    return 1
  fi
  if installation_gate "$name"; then return 0; fi
  vm_state=$(vm_process_state "$VM_DIR/vmhost.pid")
  if [ "$vm_state" = dead ]; then
    rm -f "$VM_DIR/vmhost.pid"
    emit_ok stopped "Sandbox $name is stopped; its VM did not shut down cleanly." "agent-desktop sandbox start $name"
    return 0
  fi
  if [ "$vm_state" = stale ]; then
    broken_state "Sandbox $name has a dead or foreign VM process record." "$name"
    return 0
  fi
  if [ "$vm_state" = absent ]; then
    emit_ok stopped "Sandbox $name is stopped."
    return 0
  fi
  check_guest_setup "$name"
}

clear_share_permissions() {
  [ -d "$VM_DIR/share" ] || return 0
  chmod -R u+rwX "$VM_DIR/share" 2>/dev/null || true
}

do_rm() {
  name=${1:-}
  if ! valid_name "$name"; then
    emit_fail INVALID_ARGS "The rm verb needs a valid sandbox name." "Run agent-desktop sandbox rm <name>."
    return 1
  fi
  if job_is_running; then
    emit_fail ACTION_FAILED "Sandbox $name is still installing, so it was not removed." "Run \`agent-desktop sandbox status $name\` and remove it after the install finishes or fails."
    return 1
  fi
  vm_state=$(vm_process_state "$VM_DIR/vmhost.pid")
  if [ "$vm_state" = live ]; then
    emit_fail INVALID_ARGS "Sandbox $name is running, so it was not removed." "Run agent-desktop sandbox stop $name first."
    return 1
  fi
  clear_share_permissions
  if ! rm -rf "$VM_DIR"; then
    emit_fail ACTION_FAILED "The VM files for sandbox $name could not be removed." "Run agent-desktop sandbox status $name."
    return 1
  fi
  emit_ok stopped "The VM files for sandbox $name were removed."
}

shell_quote() {
  printf "'"
  printf '%s' "$1" | sed "s/'/'\\\\''/g"
  printf "'"
}

exec_failure() {
  emit_fail ACTION_FAILED "$1" "$2"
  exit 125
}

do_exec() {
  name=${1:-}
  if ! valid_name "$name"; then
    exec_failure "The exec verb needs a valid sandbox name." "Run agent-desktop sandbox exec <name> -- <args>."
  fi
  shift
  if [ "${1:-}" != -- ]; then
    exec_failure "The exec verb needs an argument separator." "Run agent-desktop sandbox exec $name -- <args>."
  fi
  shift
  vm_state=$(vm_process_state "$VM_DIR/vmhost.pid")
  if [ "$vm_state" = dead ]; then
    rm -f "$VM_DIR/vmhost.pid"
    vm_state=absent
  fi
  if [ "$vm_state" = absent ]; then
    exec_failure "Sandbox $name is stopped." "Run agent-desktop sandbox start $name."
  fi
  if [ "$vm_state" = stale ]; then
    exec_failure "Sandbox $name has a dead or foreign VM process record." "Run agent-desktop sandbox status $name."
  fi
  if [ ! -f "$AD_SANDBOX_DIR/setup_done" ]; then
    exec_failure "Sandbox $name still needs guest setup." "Run agent-desktop sandbox status $name."
  fi
  if [ ! -f "$AD_BINARY" ]; then
    exec_failure "The host agent-desktop binary is missing." "Rebuild agent-desktop, then run sandbox exec $name again."
  fi
  user=$(guest_user || true)
  if [ -z "$user" ]; then
    exec_failure "Sandbox $name has no configured guest user." "Run agent-desktop sandbox status $name."
  fi
  ip_json=$("$VMHOST" ip "$VM_DIR" 2>/dev/null || true)
  ip=$(printf '%s' "$ip_json" | sed -n 's/.*"ip"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
  if [ -z "$ip" ]; then
    exec_failure "Sandbox $name has no reachable guest address yet." "Run agent-desktop sandbox status $name."
  fi
  if ! host_hash_line=$(shasum -a 1 "$AD_BINARY" 2>/dev/null); then
    exec_failure "The host agent-desktop binary could not be hashed." "Rebuild agent-desktop, then run sandbox exec $name again."
  fi
  host_hash=$(printf '%s\n' "$host_hash_line" | awk '{ print tolower($1) }')
  target=$user@$ip
  guest_hash_line=$(guest_transport ssh probe "$target" 'shasum -a 1 ~/.local/bin/agent-desktop' 2>/dev/null || true)
  guest_hash=$(printf '%s\n' "$guest_hash_line" | awk '{ print tolower($1) }')
  if [ "$host_hash" != "$guest_hash" ]; then
    staged_binary='~/.local/bin/agent-desktop.new'
    if ! guest_transport ssh probe "$target" 'mkdir -p ~/.local/bin' >/dev/null || \
      ! guest_transport scp long "$AD_BINARY" "$target:$staged_binary" >/dev/null || \
      ! guest_transport ssh probe "$target" "chmod 755 $staged_binary && mv -f $staged_binary ~/.local/bin/agent-desktop" >/dev/null; then
      exec_failure "agent-desktop could not be copied into sandbox $name." "Run agent-desktop sandbox status $name."
    fi
  fi
  remote_command='~/.local/bin/agent-desktop'
  for argument do
    remote_command="$remote_command $(shell_quote "$argument")"
  done
  ssh_status=0
  guest_transport ssh long "$target" "$remote_command" || ssh_status=$?
  if [ "$ssh_status" -eq 255 ]; then
    exec_failure "Remote Login could not be reached in sandbox $name." "Run agent-desktop sandbox status $name."
  fi
  return "$ssh_status"
}

require_provider_environment
if [ "$verb" = __install_job ]; then
  require_sandbox_environment
  do_install_job "$@"
  exit $?
fi

case $verb in
  check)
    do_check
    ;;
  install)
    do_install || exit $?
    ;;
  create)
    require_sandbox_environment
    do_create "${1:-}" || exit $?
    ;;
  start)
    require_sandbox_environment
    do_start "${1:-}" || exit $?
    ;;
  stop)
    require_sandbox_environment
    do_stop "${1:-}" || exit $?
    ;;
  status)
    require_sandbox_environment
    do_status "${1:-}" || exit $?
    ;;
  rm)
    require_sandbox_environment
    do_rm "${1:-}" || exit $?
    ;;
  exec)
    require_sandbox_environment
    do_exec "$@" || exit $?
    ;;
  *)
    emit_fail INVALID_ARGS "The verb is not part of the macOS VM provider contract." "Run agent-desktop install macos-vm or an agent-desktop sandbox command."
    exit 1
    ;;
esac
