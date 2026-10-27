#!/usr/bin/env bash
# cafs-test.sh — one entry point for CAFS I/O Engine tests (CI and local).
#
#   cafs-test.sh                       disk-image mode: whole suite, no privileges (what CI runs)
#   cafs-test.sh --device /dev/sdXN    ALSO run the adaptive suite + latency bench on a real
#                                      partition. The partition is DESTROYED.
#
# Options (device mode):
#   --yes            skip the typed confirmation (the Rust-side guards still apply)
#   --restore-swap   if the partition was swap, run `mkswap` again afterwards with its old label/UUID
#   -h, --help
#
# The test binaries run as YOU, not root: a temporary ACL gives your user rw on the
# partition for the duration, then removes it. Needs `setfacl` (package: acl) and sudo,
# unless the partition is already accessible to you.
#
# Refused by the engine itself, whatever this script does: mounted or swap-active
# partitions, whole disks that have partitions, devices with holders (LUKS/LVM/md), and
# anything whose start holds a filesystem other than empty / swap / CAFS.
set -euo pipefail

if [[ -t 1 ]]; then B=$'\e[1;36m'; G=$'\e[32m'; Y=$'\e[33m'; R=$'\e[1;31m'; D=$'\e[2m'; Z=$'\e[0m'; else B= G= Y= R= D= Z=; fi
usage() { sed -n '2,/^set -e/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; }
die() { echo "${R}cafs-test:${Z} $*" >&2; exit 1; }

DEV= YES=0 RESTORE=0
while (($#)); do
  case $1 in
    --device) DEV=${2:-}; shift 2 || die "--device needs a path" ;;
    --yes) YES=1; shift ;;
    --restore-swap) RESTORE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
done

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CRATE=${CAFS_CRATE_DIR:-$HERE/../Src/FUSE/Linux/x86-64/I-O_Engine}
[[ -f $CRATE/Cargo.toml ]] || die "crate not found at $CRATE (set CAFS_CRATE_DIR)"
cd "$CRATE"

echo "${B}== image mode: full suite${Z}"
cargo test

[[ -z $DEV ]] && exit 0

echo; echo "${B}== device mode: $DEV${Z}"
[[ -b $DEV ]] || die "$DEV is not a block device"
if command -v swapon >/dev/null && swapon --show=NAME --noheadings 2>/dev/null | grep -qxF "$DEV"; then
  die "$DEV is active swap. Run: sudo swapoff $DEV"
fi
if findmnt -rn -S "$DEV" >/dev/null 2>&1 || grep -q "^$DEV " /proc/mounts; then
  die "$DEV is mounted"
fi

lsblk -o NAME,SIZE,TYPE,ROTA,LOG-SEC,PHY-SEC,FSTYPE,LABEL,MOUNTPOINTS "$DEV" || true
if ((!YES)); then
  echo; echo "${R}Everything on $DEV will be overwritten.${Z}"
  read -r -p "Type the device path to continue: " ans
  [[ $ans == "$DEV" ]] || die "confirmation did not match; nothing was touched"
fi

ACL=0
cleanup() {
  ((ACL)) && sudo setfacl -x "u:$USER" "$DEV" 2>/dev/null || true
}
trap cleanup EXIT
if [[ ! -r $DEV || ! -w $DEV ]]; then
  command -v setfacl >/dev/null || die "no rw access to $DEV and setfacl is missing (install acl, or add yourself to the disk group)"
  sudo setfacl -m "u:$USER:rw" "$DEV"; ACL=1
fi

# Probe the device directly (not udev's cache, which can be stale): remembered so swap can be restored.
FSTYPE= LABEL= UUID=
while IFS='=' read -r k v; do
  case $k in TYPE) FSTYPE=$v ;; LABEL) LABEL=$v ;; UUID) UUID=$v ;; esac
done < <(blkid -p -o export "$DEV" 2>/dev/null || true)

# Start from a state the Rust-side guard accepts: swap and CAFS pass as they are;
# an empty device passes; anything else is refused there with instructions.
export CAFS_TEST_DEVICE=$DEV CAFS_TEST_DEVICE_I_AGREE=$DEV
set +e
cargo test --test adaptive --test bench -- --test-threads=1 --nocapture
RC=$?
set -e

if ((RESTORE)) && [[ $FSTYPE == swap ]]; then
  echo "${Y}restoring swap on $DEV (label='${LABEL}' uuid=${UUID})${Z}"
  args=(); [[ -n $LABEL ]] && args+=(-L "$LABEL"); [[ -n $UUID ]] && args+=(-U "$UUID")
  sudo mkswap "${args[@]}" "$DEV"
elif [[ $FSTYPE == swap ]]; then
  echo "${Y}$DEV was swap and now holds a CAFS test volume. Re-run with --restore-swap, or: sudo mkswap -L '${LABEL}' -U ${UUID} $DEV${Z}"
fi
exit $RC
