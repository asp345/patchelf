#! /bin/sh -e
SCRATCH=scratch/$(basename "$0" .sh)

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"
mkdir -p "${SCRATCH}/libsA"
mkdir -p "${SCRATCH}/libsB"

cp main "${SCRATCH}"/
cp libfoo.so libbar.so "${SCRATCH}/libsA/"
cp libfoo.so libbar.so "${SCRATCH}/libsB/"

oldRPath=$("${PATCHELF}" --print-rpath "${SCRATCH}/main")
if test -z "$oldRPath"; then oldRPath="/oops"; fi
pathA="$(pwd)/${SCRATCH}/libsA"
pathB="$(pwd)/${SCRATCH}/libsB"
"${PATCHELF}" --force-rpath --set-rpath "$oldRPath:$pathA:$pathB" "${SCRATCH}/main"

cp "${SCRATCH}"/main "${SCRATCH}/mainA"
cp "${SCRATCH}"/main "${SCRATCH}/mainB"

"${PATCHELF}" --shrink-rpath "${SCRATCH}/main"
"${PATCHELF}" --shrink-rpath --allowed-rpath-prefixes "$oldRPath:$pathA" "${SCRATCH}/mainA"
"${PATCHELF}" --shrink-rpath --allowed-rpath-prefixes "$oldRPath:$pathB" "${SCRATCH}/mainB"

check() {
    exe=$1
    mustContain=$2
    mustNotContain=$3

    rpath=$("${PATCHELF}" --print-rpath "$exe")
    echo "RPATH of $exe after: $rpath"

    if ! echo "$rpath" | grep -q "$mustContain"; then
        echo "RPATH didn't contain '$mustContain' when it should have"
        exit 1
    fi

    if echo "$rpath" | grep -q "$mustNotContain"; then
        echo "RPATH contained '$mustNotContain' when it shouldn't have"
        exit 1
    fi
}

check "${SCRATCH}/main"  "$pathA" "$pathB"
check "${SCRATCH}/mainA" "$pathA" "$pathB"
check "${SCRATCH}/mainB" "$pathB" "$pathA"
