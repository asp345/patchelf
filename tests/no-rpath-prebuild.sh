#! /bin/sh -e
set -x
ARCH="$1"
PAGESIZE=4096

if [ -z "$ARCH" ]; then
  ARCH=$(basename "$0" .sh | sed -e 's/^no-rpath-//')
fi

SCRATCH=scratch/no-rpath-$ARCH

if [ -z "$ARCH" ] || [ "$ARCH" = prebuild ] ; then
  echo "Architecture required"
  exit 1
fi

no_rpath_bin="${srcdir:?}/no-rpath-prebuild/no-rpath-$ARCH"

if [ ! -f "$no_rpath_bin" ]; then
  echo "no 'no-rpath' binary for '$ARCH' in '${srcdir:?}/no-rpath-prebuild'"
  exit 1
fi

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"

cp "$no_rpath_bin" "${SCRATCH}"/no-rpath

oldRPath=$("${PATCHELF}" --page-size "${PAGESIZE}" --print-rpath "${SCRATCH}/no-rpath")
if test -n "$oldRPath"; then exit 1; fi
"${PATCHELF}" --page-size "${PAGESIZE}" \
  --set-interpreter "$("${PATCHELF}" --page-size "${PAGESIZE}" --print-interpreter "${PATCHELF}")" \
  --set-rpath /foo:/bar:/xxxxxxxxxxxxxxx "${SCRATCH}"/no-rpath

newRPath=$("${PATCHELF}" --page-size "${PAGESIZE}" --print-rpath "${SCRATCH}/no-rpath")
if ! echo "$newRPath" | grep -q '/foo:/bar'; then
    echo "incomplete RPATH"
    exit 1
fi
