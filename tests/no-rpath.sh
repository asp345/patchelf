#! /bin/sh -e
SCRATCH=scratch/$(basename "$0" .sh)

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"

cp no-rpath "${SCRATCH}/"

oldRPath=$("${PATCHELF}" --print-rpath "${SCRATCH}/no-rpath")
if test -n "$oldRPath"; then exit 1; fi
"${PATCHELF}" \
  --set-interpreter "$("${PATCHELF}" --print-interpreter "${PATCHELF}")" \
  --set-rpath /foo:/bar:/xxxxxxxxxxxxxxx "${SCRATCH}/no-rpath"

newRPath=$("${PATCHELF}" --print-rpath "${SCRATCH}/no-rpath")
if ! echo "$newRPath" | grep -q '/foo:/bar'; then
    echo "incomplete RPATH"
    exit 1
fi

cd "${SCRATCH}" && ./no-rpath
