#! /bin/sh -e
SCRATCH=scratch/$(basename "$0" .sh)

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"

cp main "${SCRATCH}"/
SOME_PATH=$(pwd)/${SCRATCH}/some-path
printf "%s" "$SOME_PATH" >> "${SCRATCH}"/add-rpath

 "${PATCHELF}" --print-rpath "${SCRATCH}"/main | grep "$SOME_PATH" && exit 1
"${PATCHELF}" --add-rpath @"${SCRATCH}"/add-rpath "${SCRATCH}"/main
"${PATCHELF}" --print-rpath "${SCRATCH}"/main | grep "$SOME_PATH"

# should print error message and fail
"${PATCHELF}" --set-rpath @"${SCRATCH}"/does-not-exist "${SCRATCH}"/main 2>&1 | grep "cannot read"
