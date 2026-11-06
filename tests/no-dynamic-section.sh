#! /bin/sh -e

# print rpath on library with stripped dynamic section
"${PATCHELF}" --print-rpath libbig-dynstr.debug
