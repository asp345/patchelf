#! /bin/sh -e

# Growing a non-PIE executable must not move any existing segment or the
# program header table (tools like strip expect it right after the ELF
# header); new data goes into an added segment.

if test "$(uname -m)" != x86_64 || test "$(uname)" != Linux; then
    exit 77
fi

SCRATCH="scratch/$(basename "$0" .sh)"
READELF=${READELF:-readelf}

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"
cp main-no-pie "${SCRATCH}/main"

loads() {
    "${READELF}" -lW "$1" | awk '$1 == "LOAD" { print $2, $3, $5, $6, $7 }'
}
phoff() {
    "${READELF}" -hW "$1" | awk '/Start of program headers:/ { print $5 }'
}

loads "${SCRATCH}/main" > "${SCRATCH}/loads-before"
"${PATCHELF}" --set-rpath "$(printf '%0800d' 0)" "${SCRATCH}/main"
loads "${SCRATCH}/main" > "${SCRATCH}/loads-after"

# Every original segment is still there, unchanged, plus exactly one new one.
grep -vxFf "${SCRATCH}/loads-after" "${SCRATCH}/loads-before" && exit 1
test "$(wc -l < "${SCRATCH}/loads-after")" -eq "$(($(wc -l < "${SCRATCH}/loads-before") + 1))"
test "$(phoff "${SCRATCH}/main")" -eq 64

exitCode=0
LD_LIBRARY_PATH=. "${SCRATCH}/main" || exitCode=$?
test "${exitCode}" -eq 46
