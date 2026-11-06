#! /bin/sh -e

# phdr-corruption.so has a .dynamic section but no PT_DYNAMIC, so the dynamic
# loader would never read a run path from it. patchelf must refuse cleanly
# and leave the file untouched rather than write a corrupt one.

PATCHELF="${PATCHELF:?}"
SONAME="phdr-corruption.so"
SCRATCH="scratch/$(basename "$0" .sh)"
SCRATCH_SO="${SCRATCH}/${SONAME}"

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"
cp "${SONAME}" "${SCRATCH}"

if "${PATCHELF}" --set-rpath "$(pwd)" "${SCRATCH_SO}" 2> "${SCRATCH}/err"; then
    echo "ERROR: expected patchelf to refuse a file without PT_DYNAMIC"
    exit 1
fi
grep -q "cannot find the dynamic section" "${SCRATCH}/err"
cmp "${SONAME}" "${SCRATCH_SO}"
