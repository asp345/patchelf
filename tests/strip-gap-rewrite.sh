#! /bin/sh -e

if test "$(uname -m)" != x86_64 || test "$(uname)" != Linux; then
    exit 77
fi

SCRATCH="scratch/$(basename "$0" .sh)"
READELF=${READELF:-readelf}
STRIP=${STRIP:-strip}

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"
for pageSize in 8192 65536; do
    cp main-no-pie-rpath "${SCRATCH}/main"

    # The oversized page leaves file padding for strip to remove. strip must
    # understand the result without complaint.
    "${PATCHELF}" --page-size "${pageSize}" --set-rpath "$(printf '%0800d' 0)" "${SCRATCH}/main"
    "${STRIP}" "${SCRATCH}/main" 2> "${SCRATCH}/strip.err"
    if test -s "${SCRATCH}/strip.err"; then
        cat "${SCRATCH}/strip.err"
        exit 1
    fi

    # With a large virtual gap, growth must still reserve enough file space
    # for a replacement that needs more than one additional page. This also
    # passes on master, but guards against ignoring file growth when reusing
    # the header LOAD introduced by this fix.
    size=1600
    if test "${pageSize}" -eq 65536; then
        size=16000
    fi
    "${PATCHELF}" --set-rpath "$(printf "%0${size}d" 0)" "${SCRATCH}/main"

    exitCode=0
    LD_LIBRARY_PATH=. "${SCRATCH}/main" || exitCode=$?
    test "${exitCode}" -eq 46
done
