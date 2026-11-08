#!/bin/sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/ad macos vm provider.XXXXXX")
cleanup() {
  for pid in "${source_pid:-}" "${foreign_pid:-}" "${vm_pid:-}" "${job_pid:-}"; do
    [ -n "$pid" ] || continue
    kill "$pid" 2>/dev/null || true
  done
  chmod -R u+rwX "$tmp" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup 0
trap 'exit 1' 1 2 3 15
mkdir -p "$tmp/bin" "$tmp/provider/build" "$tmp/sandboxes/demo"

AD_SANDBOX_DIR=$tmp/sandboxes/demo
AD_PROVIDER_DIR=$tmp/provider
AD_BINARY=$tmp/agent-desktop
VMHOST=$tmp/bin/vmhost
FAKE_LOG=$tmp/calls
FAKE_HV_SUPPORT=1
FAKE_SWIFTC_FOUND=1
FAKE_GUEST_SHA1=2222222222222222222222222222222222222222
FAKE_EXEC_BUDGET=300
FAKE_PS_COMM=/opt/agent-desktop/build/vmhost
FAKE_PS_BUNDLE=$AD_SANDBOX_DIR/vm
FAKE_PS_COMMAND=
export AD_SANDBOX_DIR AD_PROVIDER_DIR AD_BINARY VMHOST FAKE_LOG
export FAKE_HV_SUPPORT FAKE_SWIFTC_FOUND FAKE_GUEST_SHA1 FAKE_EXEC_BUDGET
export FAKE_PS_BUNDLE FAKE_PS_COMM FAKE_PS_COMMAND

printf '#!/bin/sh\n' > "$AD_PROVIDER_DIR/guest-setup.sh"
printf 'binary\n' > "$AD_BINARY"

cat > "$tmp/bin/uname" <<'FAKE'
#!/bin/sh
printf '%s\n' arm64
FAKE
cat > "$tmp/bin/sw_vers" <<'FAKE'
#!/bin/sh
printf '%s\n' 14.0
FAKE
cat > "$tmp/bin/sysctl" <<'FAKE'
#!/bin/sh
printf '%s\n' "${FAKE_HV_SUPPORT:-1}"
FAKE
cat > "$tmp/bin/xcrun" <<'FAKE'
#!/bin/sh
if [ "${1:-}" = --find ]; then
  [ "${FAKE_SWIFTC_FOUND:-1}" = 1 ] || exit 1
  printf '%s\n' /usr/bin/swiftc
  exit 0
fi
exit 2
FAKE
cat > "$tmp/bin/codesign" <<'FAKE'
#!/bin/sh
printf 'com.apple.security.virtualization\n'
FAKE
cat > "$tmp/bin/vmhost" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN vmhost\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
case ${1:-} in
  install) exit 1 ;;
  clone)
    mkdir -p "$3/share"
    : > "$3/Disk.img"
    : > "$3/AuxiliaryStorage"
    : > "$3/HardwareModel"
    : > "$3/MachineIdentifier"
    : > "$3/config.json"
    ;;
  ip) printf '{"ok":true,"ip":"192.0.2.2"}\n' ;;
  *) exit 2 ;;
esac
FAKE
cat > "$tmp/bin/ssh-keygen" <<'FAKE'
#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  if [ "$1" = -f ]; then shift; key=$1; fi
  shift
done
printf 'private key\n' > "$key"
printf 'public key\n' > "$key.pub"
FAKE
cat > "$tmp/bin/curl" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN curl\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
exit 1
FAKE
cat > "$tmp/bin/ssh" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN ssh\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
remote=
batch=
connect_timeout=
alive_interval=
alive_count=
for arg in "$@"; do
  remote=$arg
  case $arg in
    BatchMode=yes) batch=1 ;;
    ConnectTimeout=*) connect_timeout=${arg#*=} ;;
    ServerAliveInterval=*) alive_interval=${arg#*=} ;;
    ServerAliveCountMax=*) alive_count=${arg#*=} ;;
  esac
done
if [ -z "$batch" ]; then sleep 40; exit 255; fi
case ${FAKE_SSH_BEHAVIOR:-echo} in
  unreachable)
    if [ -z "$connect_timeout" ]; then sleep 40; fi
    exit 255
    ;;
  silent)
    if [ -z "$alive_interval" ] || [ -z "$alive_count" ]; then sleep 40; exit 255; fi
    sleep "$((alive_interval * alive_count))"
    exit 255
    ;;
  quiet)
    cutoff=0
    if [ -n "$alive_interval" ] && [ -n "$alive_count" ]; then
      cutoff=$((alive_interval * alive_count))
    fi
    case $remote in
      *'shasum -a 1'*|*'mkdir -p'*|*'chmod 755'*) ;;
      *)
        if [ "$cutoff" -gt 0 ] && [ "$cutoff" -lt "${FAKE_EXEC_BUDGET:-300}" ]; then
          exit 255
        fi
        printf 'guest output\n'
        exit 0
        ;;
    esac
    ;;
esac
case $remote in
  *'shasum -a 1'*) printf '%s  agent-desktop\n' "${FAKE_GUEST_SHA1:-}" ;;
  *) printf '{"ok":true,"command":"guest"}\n' ;;
esac
FAKE
cat > "$tmp/bin/scp" <<'FAKE'
#!/bin/sh
set -eu
{
  printf 'BEGIN scp\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$FAKE_LOG"
batch=
connect_timeout=
for arg in "$@"; do
  case $arg in
    BatchMode=yes) batch=1 ;;
    ConnectTimeout=*) connect_timeout=${arg#*=} ;;
  esac
done
if [ -z "$batch" ] || [ -z "$connect_timeout" ]; then
  sleep 40
  exit 1
fi
FAKE
cat > "$tmp/bin/ps" <<'FAKE'
#!/bin/sh
case " $* " in
  *" command="*) printf '%s\n' "${FAKE_PS_COMMAND:-/opt/agent-desktop/build/vmhost run $FAKE_PS_BUNDLE}" ;;
  *) printf '%s\n' "${FAKE_PS_COMM:-/opt/agent-desktop/build/vmhost}" ;;
esac
FAKE
chmod 0755 "$tmp/bin/"*
PATH=$tmp/bin:$PATH
export PATH

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

run_provider() {
  sh "$here/provider.sh" "$@"
}

run_provider_with() {
  setting=$1
  value=$2
  shift 2
  env "$setting=$value" sh "$here/provider.sh" "$@"
}

wait_for_exit() {
  wait_tries=0
  while kill -0 "$1" 2>/dev/null && [ "$wait_tries" -lt 100 ]; do
    sleep 0.1
    wait_tries=$((wait_tries + 1))
  done
}

assert_field() {
  actual=$(python3 -c 'import json,sys; print(json.loads(open(sys.argv[1]).readlines()[-1])[sys.argv[2]])' "$1" "$2")
  [ "$actual" = "$3" ] || fail "$4: expected $2=[$3] but saw [$actual]"
}

run_ok() {
  status=0
  run_provider "$@" > "$tmp/out" 2> "$tmp/err" || status=$?
  [ "$status" = 0 ] || fail "provider $1 exited $status: $(cat "$tmp/out" "$tmp/err")"
}

run_ok_with() {
  setting=$1
  value=$2
  verb=$3
  shift 3
  status=0
  run_provider_with "$setting" "$value" "$verb" "$@" > "$tmp/out" 2> "$tmp/err" || status=$?
  [ "$status" = 0 ] || fail "provider $verb exited $status: $(cat "$tmp/out" "$tmp/err")"
}

FAKE_HV_SUPPORT=0
run_ok check
assert_field "$tmp/out" state unavailable 'a Mac that cannot run VMs'
FAKE_HV_SUPPORT=1
FAKE_SWIFTC_FOUND=0
run_ok check
assert_field "$tmp/out" state unavailable 'missing Command Line Tools'
FAKE_SWIFTC_FOUND=1

printf '#!/bin/sh\nexit 0\n' > "$AD_PROVIDER_DIR/build/vmhost"
chmod 0755 "$AD_PROVIDER_DIR/build/vmhost"
: > "$FAKE_LOG"
run_ok install
assert_field "$tmp/out" state ready 'a built and signed VM host'

source_dir=$tmp/sandboxes/source
mkdir -p "$source_dir/vm/share"
: > "$source_dir/vm/Disk.img"
: > "$source_dir/vm/AuxiliaryStorage"
: > "$source_dir/vm/HardwareModel"
: > "$source_dir/vm/MachineIdentifier"
: > "$source_dir/vm/config.json"
printf '#!/bin/sh\n' > "$source_dir/vm/share/guest-setup.sh"
printf 'source public key\n' > "$source_dir/vm/share/id_ed25519.pub"
printf 'binary\n' > "$source_dir/vm/share/agent-desktop"
printf 'source setup marker\n' > "$source_dir/setup_done"
printf 'source key\n' > "$source_dir/id_ed25519"
printf 'source public key\n' > "$source_dir/id_ed25519.pub"
printf 'source hosts\n' > "$source_dir/known_hosts"
printf 'guest\n' > "$source_dir/guest_user"
: > "$FAKE_LOG"
AD_SET_USER=requested
export AD_SET_USER
run_ok_with AD_SET_FROM source create demo
unset AD_SET_USER
python3 - "$tmp/out" "$FAKE_LOG" "$source_dir" "$AD_SANDBOX_DIR" <<'PY'
import json
import pathlib
import sys

reply, log, source, target = map(pathlib.Path, sys.argv[1:])
assert (
    json.loads(reply.read_text().splitlines()[-1])["state"] == "stopped"
    and "BEGIN curl" not in log.read_text().splitlines()
    and "clone" in log.read_text().splitlines()
    and all((target / name).read_bytes() == (source / name).read_bytes()
            for name in ("id_ed25519", "known_hosts", "setup_done", "guest_user"))
), "create did not clone a set-up stopped source without downloading"
PY

sleep 30 &
source_pid=$!
printf '%s\n' "$source_pid" > "$source_dir/vm/vmhost.pid"
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $source_dir/vm"
rm -rf "$AD_SANDBOX_DIR"
mkdir -p "$AD_SANDBOX_DIR"
status=0
run_provider_with AD_SET_FROM source create demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "create from a running source exited $status"
kill -0 "$source_pid" 2>/dev/null || fail 'create signalled the running source'
kill "$source_pid" 2>/dev/null || true
wait "$source_pid" 2>/dev/null || true
source_pid=
rm -f "$source_dir/vm/vmhost.pid"
FAKE_PS_COMMAND=

user_ipsw=$tmp/user.ipsw
printf 'user image\n' > "$user_ipsw"
run_ok_with AD_SET_IPSW "$user_ipsw" create demo
job_pid=$(cat "$AD_SANDBOX_DIR/vm/install.pid")
wait_for_exit "$job_pid"
job_pid=
[ -f "$user_ipsw" ] || fail 'a failed installation deleted the user-supplied image'

mkdir -p "$AD_SANDBOX_DIR/vm"
sleep 30 &
foreign_pid=$!
printf '%s\n' "$foreign_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
FAKE_PS_COMM=/usr/bin/sleep
run_ok stop demo
kill -0 "$foreign_pid" 2>/dev/null || fail 'stop signalled a pid whose command was not vmhost'
[ -f "$AD_SANDBOX_DIR/vm/vmhost.pid" ] || fail 'stop discarded a live foreign VM process record'
kill "$foreign_pid" 2>/dev/null || true
wait "$foreign_pid" 2>/dev/null || true
foreign_pid=
sleep 30 &
foreign_pid=$!
printf '%s\n' "$foreign_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
FAKE_PS_COMM=/opt/agent-desktop/build/vmhost
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $tmp/sandboxes/other/vm"
run_ok stop demo
assert_field "$tmp/out" state broken 'stop of a foreign VM host pid'
kill -0 "$foreign_pid" 2>/dev/null || fail 'stop signalled a vmhost pid that runs another bundle'
[ -f "$AD_SANDBOX_DIR/vm/vmhost.pid" ] || fail 'stop discarded a foreign VM process record for another bundle'
kill "$foreign_pid" 2>/dev/null || true
wait "$foreign_pid" 2>/dev/null || true
foreign_pid=
FAKE_PS_COMMAND=

mkdir -p "$AD_SANDBOX_DIR/vm/share"
: > "$AD_SANDBOX_DIR/vm/Disk.img"
: > "$AD_SANDBOX_DIR/vm/AuxiliaryStorage"
: > "$AD_SANDBOX_DIR/vm/HardwareModel"
: > "$AD_SANDBOX_DIR/vm/MachineIdentifier"
: > "$AD_SANDBOX_DIR/vm/config.json"
printf '#!/bin/sh\n' > "$AD_SANDBOX_DIR/vm/share/guest-setup.sh"
printf 'public key\n' > "$AD_SANDBOX_DIR/vm/share/id_ed25519.pub"
cp "$AD_BINARY" "$AD_SANDBOX_DIR/vm/share/agent-desktop"
rm -f "$AD_SANDBOX_DIR/vm/install.pid" "$AD_SANDBOX_DIR/vm/install.done"
printf '999999999\n' > "$AD_SANDBOX_DIR/vm/vmhost.pid"
run_ok status demo
assert_field "$tmp/out" state stopped 'a dead VM host pid'
[ ! -f "$AD_SANDBOX_DIR/vm/vmhost.pid" ] || fail 'status kept the dead VM process record'
printf '999999999\n' > "$AD_SANDBOX_DIR/vm/vmhost.pid"
: > "$FAKE_LOG"
run_provider start demo > "$tmp/out" 2> "$tmp/err" || true
grep -qx 'run' "$FAKE_LOG" || fail "start did not launch the VM host over a dead record: $(cat "$tmp/out" "$tmp/err")"

printf '1000000000\n' > "$AD_SANDBOX_DIR/vm/download.size"
mkfile -n 340000000 "$AD_SANDBOX_DIR/vm/latest.ipsw"
printf '%s\n' "$AD_SANDBOX_DIR/vm/latest.ipsw" > "$AD_SANDBOX_DIR/vm/download.path"
sleep 30 &
job_pid=$!
printf '%s\n' "$job_pid" > "$AD_SANDBOX_DIR/vm/install.pid"
run_ok_with FAKE_PS_COMMAND "$here/provider.sh __install_job download" status demo
grep -q 'is downloading the macOS image: 0.3 of 1.0 GB (34%)\.' "$tmp/out" || \
  fail "status did not report download progress: $(cat "$tmp/out")"
kill "$job_pid" 2>/dev/null || true
wait "$job_pid" 2>/dev/null || true
job_pid=
rm -f "$AD_SANDBOX_DIR/vm/install.pid" "$AD_SANDBOX_DIR/vm/download.size" "$AD_SANDBOX_DIR/vm/download.path"

sleep 30 &
job_pid=$!
printf '%s\n' "$job_pid" > "$AD_SANDBOX_DIR/vm/install.pid"
status=0
run_provider_with FAKE_PS_COMMAND "$here/provider.sh __install_job download" rm demo > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 1 ] || fail "rm while installing exited $status"
assert_field "$tmp/out" code ACTION_FAILED 'rm while installing'
kill -0 "$job_pid" 2>/dev/null || fail 'rm killed the install job'
[ -d "$AD_SANDBOX_DIR/vm" ] || fail 'rm removed the bundle while the install job was running'
kill "$job_pid" 2>/dev/null || true
wait "$job_pid" 2>/dev/null || true
job_pid=
rm -f "$AD_SANDBOX_DIR/vm/install.pid"
FAKE_PS_COMMAND=

sleep 30 &
vm_pid=$!
printf '%s\n' "$vm_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
: > "$AD_SANDBOX_DIR/setup_done"
weird='semi; double "quoted" and '\''single'\'' $(touch /tmp/should-not-run)'
: > "$FAKE_LOG"
run_provider exec demo -- "$weird" > "$tmp/out" 2> "$tmp/err"
python3 - "$FAKE_LOG" "$weird" <<'PY'
import shlex
import sys

blocks = {}
current = None
for line in open(sys.argv[1]):
    line = line.rstrip("\n")
    if line.startswith("BEGIN ") and current is None:
        current = [line[len("BEGIN "):], []]
    elif line == "END" and current is not None:
        blocks.setdefault(current[0], []).append(current[1])
        current = None
    elif current is not None:
        current[1].append(line)
ssh = blocks.get("ssh", [])
scp = blocks.get("scp", [])
if not ssh:
    sys.exit("no ssh command was recorded")
if shlex.split(ssh[-1][-1]) != ["~/.local/bin/agent-desktop", sys.argv[2]]:
    sys.exit("exec argument was not preserved as one quoted word: %r" % ssh[-1][-1])
commands = [shlex.split(block[-1]) for block in ssh if block]
if ["mkdir", "-p", "~/.local/bin"] not in commands:
    sys.exit("exec did not create ~/.local/bin in the guest")
if not scp or not scp[-1][-1].endswith(":~/.local/bin/agent-desktop.new"):
    sys.exit("exec did not copy onto a staging file: %r" % scp)
if not [words for words in commands
        if words[:3] == ["chmod", "755", "~/.local/bin/agent-desktop.new"]
        and words[-2:] == ["~/.local/bin/agent-desktop.new", "~/.local/bin/agent-desktop"]]:
    sys.exit("exec did not move the staged binary onto ~/.local/bin/agent-desktop")
PY
kill "$vm_pid" 2>/dev/null || true
wait "$vm_pid" 2>/dev/null || true
vm_pid=

sleep 30 &
vm_pid=$!
printf '%s\n' "$vm_pid" > "$AD_SANDBOX_DIR/vm/vmhost.pid"
FAKE_PS_COMMAND="/opt/agent-desktop/build/vmhost run $AD_SANDBOX_DIR/vm"
rm -f "$AD_SANDBOX_DIR/setup_done"
probe_started=$(date +%s)
run_ok_with FAKE_SSH_BEHAVIOR silent status demo
probe_elapsed=$(( $(date +%s) - probe_started ))
assert_field "$tmp/out" state needs_setup 'status against a connected but silent guest'
[ "$probe_elapsed" -lt 25 ] || \
  fail "status blocked ${probe_elapsed}s on a silent guest instead of reporting a state inside the 30s status budget"

probe_started=$(date +%s)
run_ok_with FAKE_SSH_BEHAVIOR unreachable status demo
probe_elapsed=$(( $(date +%s) - probe_started ))
assert_field "$tmp/out" state needs_setup 'status against an unreachable guest'
[ "$probe_elapsed" -lt 25 ] || \
  fail "status blocked ${probe_elapsed}s on an unreachable guest instead of using a bounded connect timeout"

: > "$AD_SANDBOX_DIR/setup_done"
FAKE_SCP_BEHAVIOR=prompt
export FAKE_SCP_BEHAVIOR
run_ok_with FAKE_SSH_BEHAVIOR quiet exec demo -- snapshot
unset FAKE_SCP_BEHAVIOR
grep -qx 'guest output' "$tmp/out" || \
  fail "exec did not carry a quiet remote command past the status probe bound: $(cat "$tmp/out" "$tmp/err")"

kill "$vm_pid" 2>/dev/null || true
wait "$vm_pid" 2>/dev/null || true
vm_pid=
FAKE_PS_COMMAND=
rm -f "$AD_SANDBOX_DIR/setup_done"

printf '999999999\n' > "$AD_SANDBOX_DIR/vm/vmhost.pid"
status=0
run_provider exec demo -- snapshot > "$tmp/out" 2> "$tmp/err" || status=$?
[ "$status" = 125 ] || fail "exec on a stopped sandbox exited $status"
grep -q 'Sandbox demo is stopped\.' "$tmp/out" || fail 'exec on a stopped sandbox did not report it'

printf 'provider tests passed\n'
