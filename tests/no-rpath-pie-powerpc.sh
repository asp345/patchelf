#! /bin/sh -e
set -x
SCRATCH=scratch/no-rpath-pie-powerpc
READELF=${READELF:-readelf}

no_rpath_bin="${srcdir:?}/no-rpath-prebuild/no-rpath-pie-powerpc"

if [ ! -f "$no_rpath_bin" ]; then
  echo "no 'no-rpath' binary for '$ARCH' in '${srcdir:?}/no-rpath-prebuild'"
  exit 1
fi

rm -rf ${SCRATCH}
mkdir -p ${SCRATCH}

cp "$no_rpath_bin" ${SCRATCH}/no-rpath

oldRPath=$("${PATCHELF}" --print-rpath ${SCRATCH}/no-rpath)
if test -n "$oldRPath"; then exit 1; fi
"${PATCHELF}" \
  --set-interpreter "$("${PATCHELF}" --print-interpreter "${PATCHELF}")" \
  --set-rpath /foo:/bar:/xxxxxxxxxxxxxxx ${SCRATCH}/no-rpath

newRPath=$("${PATCHELF}" --print-rpath ${SCRATCH}/no-rpath)
if ! echo "$newRPath" | grep -q '/foo:/bar'; then
    echo "incomplete RPATH"
    exit 1
fi

# Tests for powerpc PIE endianness regressions
readelfData=$(${READELF} -l ${SCRATCH}/no-rpath 2>&1)

if [ "$(echo "$readelfData" | grep -c "PHDR")" != 1 ]; then
  # Triggered if PHDR errors appear on stderr
  echo "Unexpected number of occurences of PHDR in readelf results"
  exit 1
fi

# PT_PHDR must give the address at which a LOAD segment maps the program
# header table. A byte-swapped address would match no segment.
phdr=$(echo "$readelfData" | awk '$1 == "PHDR" { print $2, $3 }')
phdrOff=$((${phdr% *}))
phdrAddr=$((${phdr#* }))
mapped=$(echo "$readelfData" | awk '$1 == "LOAD" { print $2, $3, $5 }' | while read -r off addr size; do
  if [ "$phdrOff" -ge $((off)) ] && [ "$phdrOff" -lt $((off + size)) ]; then
    echo $((addr + phdrOff - off))
  fi
done)
if [ "$mapped" != "$phdrAddr" ]; then
  echo "PT_PHDR address [$phdrAddr] does not match its LOAD mapping [$mapped]"
  exit 1
fi

echo "$readelfData" | grep "LOAD" | while read -r line ; do
  align=$(echo "$line" | awk '{print $NF}')
  if [ "$align" != "0x10000" ]; then
    # Triggered if the target arch was not detected properly
    echo "Unexpected Align for LOAD segment, expected [0x10000] got [$align]"
    echo "Load segment: [$line]"
    exit 1
  fi
done

