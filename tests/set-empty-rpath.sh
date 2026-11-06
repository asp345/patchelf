#! /bin/sh -e
SCRATCH=scratch/$(basename "$0" .sh)

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"

cp simple "${SCRATCH}"/simple

"${PATCHELF}" --force-rpath --set-rpath "" "${SCRATCH}/simple"

"${SCRATCH}"/simple
