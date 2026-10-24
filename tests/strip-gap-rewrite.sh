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

    ../src/patchelf --page-size "${pageSize}" --set-rpath "$(printf '%0800d' 0)" "${SCRATCH}/main"
    "${STRIP}" "${SCRATCH}/main"

    size=1600
    if test "${pageSize}" -eq 65536; then
        size=16000
    fi
    ../src/patchelf --set-rpath "$(printf "%0${size}d" 0)" "${SCRATCH}/main"

    exitCode=0
    LD_LIBRARY_PATH=. "${SCRATCH}/main" || exitCode=$?
    test "${exitCode}" -eq 46
done
