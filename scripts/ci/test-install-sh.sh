#!/usr/bin/env bash
# Tests of the dependency and resource checks of install.sh, without installing anything: the script is sourced with
# GOW_INSTALL_SOURCE_ONLY=1, so only its functions are defined. Runs on any Linux, and in Git Bash.
#
#   bash scripts/ci/test-install-sh.sh
set -u

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
failures=0
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1"; failures=$((failures + 1)); }
expect() {   # expect DESCRIPTION EXPECTED ACTUAL
    if [ "$2" = "$3" ]; then pass "$1"; else fail "$1 (expected '$2', got '$3')"; fi
}

bash -n "$ROOT/install.sh" && pass "install.sh has no syntax errors" || fail "install.sh has a syntax error"

export GOW_INSTALL_SOURCE_ONLY=1
# shellcheck source=/dev/null
. "$ROOT/install.sh"
set +e +o pipefail   # the sourced script turns them on; the tests check exit codes themselves

os_release() {   # os_release ID [ID_LIKE]
    printf 'NAME="x"\nID=%s\n' "$1" > "$scratch/os-release"
    [ -n "${2:-}" ] && printf 'ID_LIKE="%s"\n' "$2" >> "$scratch/os-release"
    OS_RELEASE="$scratch/os-release"
}

echo "distribution family"
os_release ubuntu debian;           expect "Ubuntu is debian" debian "$(distro_family)"
os_release linuxmint "ubuntu debian"; expect "Mint is debian" debian "$(distro_family)"
os_release debian;                  expect "Debian is debian" debian "$(distro_family)"
os_release fedora;                  expect "Fedora is fedora" fedora "$(distro_family)"
os_release nobara "rhel centos fedora"; expect "Nobara is fedora" fedora "$(distro_family)"
os_release arch;                    expect "Arch is arch" arch "$(distro_family)"
os_release cachyos arch;            expect "CachyOS is arch" arch "$(distro_family)"
os_release manjaro arch;            expect "Manjaro is arch" arch "$(distro_family)"
os_release nixos;                   expect "NixOS is unknown" unknown "$(distro_family)"
OS_RELEASE="$scratch/does-not-exist"; expect "a missing os-release is unknown" unknown "$(distro_family)"

echo "packages"
for family in arch debian fedora; do
    for key in $CHECKS; do
        [ -n "$(package_for "$family" "$key")" ] || fail "no $family package for $key"
    done
done
pass "every check has a package in every family"
expect "debian g++" build-essential "$(package_for debian g++)"
expect "debian clang++" clang "$(package_for debian clang++)"
expect "fedora clang++" clang "$(package_for fedora clang++)"
expect "arch clang++" clang "$(package_for arch clang++)"
expect "fedora ninja" ninja-build "$(package_for fedora ninja)"
expect "arch ninja" ninja "$(package_for arch ninja)"
expect "debian ALSA" libasound2-dev "$(package_for debian alsa)"
expect "fedora Wayland" wayland-devel "$(package_for fedora wayland-client)"
expect "git is git" git "$(package_for arch git)"
expect "the command on debian" "sudo apt install a b" "$(install_command debian a b)"
expect "the command on arch" "sudo pacman -S --needed a b" "$(install_command arch a b)"
expect "the command on fedora" "sudo dnf install a b" "$(install_command fedora a b)"

echo "what is missing"
fake="$scratch/bin"; libs="$scratch/libs"
mkdir -p "$fake" "$libs"
for program in cmake ninja g++ git pkg-config; do printf '#!/bin/bash\nexit 0\n' > "$fake/$program"; chmod +x "$fake/$program"; done
# A pkg-config that knows the libraries named by the files in $libs.
printf '#!/bin/bash\n[ "$1" = "--exists" ] && [ -e "%s/$2" ]\n' "$libs" > "$fake/pkg-config"
for lib in gl x11 xrandr xinerama xcursor xi xkbcommon wayland-client alsa; do : > "$libs/$lib"; done

PATH="$fake" find_missing debian
expect "nothing is missing when everything is there" 0 "${#MISSING_CHECKS[@]}"

rm "$libs/alsa" "$libs/xi"
PATH="$fake" find_missing debian
expect "missing libraries are listed" "xi alsa" "${MISSING_CHECKS[*]}"
expect "and mapped to their packages" "libxi-dev libasound2-dev" "${MISSING_PACKAGES[*]}"

rm "$fake/ninja" "$fake/g++"
PATH="$fake" find_missing fedora
expect "missing programs come first" "ninja g++ xi alsa" "${MISSING_CHECKS[*]}"
expect "with the Fedora names" "ninja-build gcc-c++ libXi-devel alsa-lib-devel" "${MISSING_PACKAGES[*]}"

rm "$fake/pkg-config"
PATH="$fake" find_missing arch
expect "without pkg-config every library counts as missing" \
    "ninja g++ pkg-config gl x11 xrandr xinerama xcursor xi xkbcommon wayland-client alsa" "${MISSING_CHECKS[*]}"
expect "and its package is asked once" 1 "$(printf '%s\n' "${MISSING_PACKAGES[@]}" | grep -c '^base-devel$')"

echo "the report the launcher reads"
report="$(PATH="$fake" find_missing debian; report_missing debian)"
case "$report" in
    *"[missing] ninja g++ pkg-config"*"[packages] ninja-build build-essential pkg-config"*"[install-command] sudo apt install ninja-build build-essential pkg-config"*)
        pass "the three lines are there" ;;
    *) fail "the report is not what the launcher expects: $report" ;;
esac

echo "compiler selection (clang++)"
set_compiler clang
expect "clang sets CC_BIN" clang "$CC_BIN"
expect "clang sets CXX_BIN" clang++ "$CXX_BIN"
expect "clang updates CHECKS" "cmake ninja clang++ git pkg-config gl x11 xrandr xinerama xcursor xi xkbcommon wayland-client alsa" "$CHECKS"
PATH="$fake" find_missing fedora
expect "missing clang++ maps to clang package on fedora" "ninja-build clang pkgconf-pkg-config mesa-libGL-devel libX11-devel libXrandr-devel libXinerama-devel libXcursor-devel libXi-devel libxkbcommon-devel wayland-devel alsa-lib-devel" "${MISSING_PACKAGES[*]}"
set_compiler g++
expect "restoring g++ resets CXX_BIN" g++ "$CXX_BIN"

echo "disk and memory"
free_gb() { echo "$FAKE_FREE"; }
run_check() {   # run_check FREE MEM [extracted] -> "status|output"
    local out status
    out="$( FAKE_FREE="$1" MEM_GB="$2"; export FAKE_FREE MEM_GB; check_resources /somewhere "${3:-0}" 2>&1 )"
    status=$?
    printf '%s|%s' "$status" "$out"
}
case "$(run_check 100 16)" in 0\|) pass "plenty of everything is silent" ;; *) fail "plenty of everything should be silent: $(run_check 100 16)" ;; esac
case "$(run_check 10 16)" in 1\|*"Only 10 GB free"*) pass "too little disk stops the installation" ;; *) fail "too little disk: $(run_check 10 16)" ;; esac
case "$(run_check 10 16 1)" in 0\|*"already extracted"*) pass "too little disk only warns once the disc is extracted" ;; *) fail "extracted: $(run_check 10 16 1)" ;; esac
case "$(run_check 100 2)" in 1\|*"at least 4 GB"*) pass "too little RAM stops the installation" ;; *) fail "too little RAM: $(run_check 100 2)" ;; esac
case "$(run_check 100 6)" in 0\|*"8 GB are recommended"*) pass "little RAM only warns" ;; *) fail "little RAM: $(run_check 100 6)" ;; esac
case "$(run_check '' 16)" in 0\|*"Could not read the free space"*) pass "an unreadable disk only warns" ;; *) fail "unreadable disk: $(run_check '' 16)" ;; esac
case "$( GOW_SKIP_RESOURCE_CHECK=1 run_check 1 1 )" in 0\|) pass "GOW_SKIP_RESOURCE_CHECK skips both" ;; *) fail "skip: $(GOW_SKIP_RESOURCE_CHECK=1 run_check 1 1)" ;; esac

echo "build jobs: cores, RAM in GB"
# The reserve is 2 from 8 cores, 1 from 4, 0 below; one job per 2 GB; never below 1.
expect "16 cores, 32 GB" 14 "$(build_jobs 16 32)"
expect "8 cores, 32 GB" 6 "$(build_jobs 8 32)"
expect "6 cores, 16 GB" 5 "$(build_jobs 6 16)"
expect "4 cores, 16 GB" 3 "$(build_jobs 4 16)"
expect "2 cores, 16 GB" 2 "$(build_jobs 2 16)"
expect "16 cores, 8 GB (memory bound)" 4 "$(build_jobs 16 8)"
expect "1 core, 4 GB" 1 "$(build_jobs 1 4)"
expect "12 cores, 16 GB" 8 "$(build_jobs 12 16)"
expect "7 cores, 64 GB" 6 "$(build_jobs 7 64)"
expect "3 cores, 64 GB" 3 "$(build_jobs 3 64)"
expect "4 cores, 2 GB" 1 "$(build_jobs 4 2)"
expect "8 cores, 1 GB (less than 2 GB still gets one)" 1 "$(build_jobs 8 1)"

echo "low priority"
printf '#!/bin/bash\necho "nice $*" >> "%s/calls"\nshift 2\nexec "$@"\n' "$scratch" > "$fake/nice"
printf '#!/bin/bash\necho "ionice $*" >> "%s/calls"\nshift 2\nexec "$@"\n' "$scratch" > "$fake/ionice"
chmod +x "$fake/nice" "$fake/ionice"
: > "$scratch/calls"
LOW_PRIORITY=1; unset IONICE_OK
out="$(PATH="$fake:$PATH" low_priority echo hello)"
expect "the command still runs and keeps its output" hello "$out"
expect "it runs under ionice idle and nice 10" "ionice -c 3 true
ionice -c 3 nice -n 10 echo hello
nice -n 10 echo hello" "$(cat "$scratch/calls")"
: > "$scratch/calls"; IONICE_OK=0
PATH="$fake:$PATH" low_priority true
expect "without a working ionice only nice is used" "nice -n 10 true" "$(cat "$scratch/calls")"
: > "$scratch/calls"; LOW_PRIORITY=0
out="$(PATH="$fake:$PATH" low_priority echo plain)"
expect "--normal-priority runs the command directly" "plain|" "$out|$(cat "$scratch/calls")"

if [ "$failures" -gt 0 ]; then
    printf '\n%s test(s) failed\n' "$failures"
    exit 1
fi
printf '\nall passed\n'
