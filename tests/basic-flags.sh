#! /bin/sh -e

set -x
"${PATCHELF}" --version | grep -q patchelf
"${PATCHELF}" --help 2>&1 | grep -q patchelf
"${PATCHELF}" 2>&1 | grep -q patchelf
