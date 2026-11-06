#! /bin/sh -e

# Build a small program for other architectures, patch it in several ways and
# run it under qemu-user. Architectures whose cross compiler or qemu is not
# installed are skipped; the test is skipped if none are available.
#
# main-* has a 16 MiB .bss, so its new segment cannot sit where old kernels
# and older qemu look for the program headers; the run checks that they are
# still found.

SCRATCH=scratch/$(basename "$0" .sh)
PATCHELF=$(readlink -f "${PATCHELF:?}")
READELF=${READELF:-readelf}

rm -rf "${SCRATCH}"
mkdir -p "${SCRATCH}"
TOP=$(pwd)

expected="Hello World
This is foo()!
This is bar()!
Result is 46"

tested=0
failed=0

fail() {
    echo "FAIL: $*"
    failed=$((failed + 1))
}

for spec in "aarch64-linux-gnu aarch64" "i686-linux-gnu i386" "arm-linux-gnueabihf arm" \
            "powerpc-linux-gnu ppc" "s390x-linux-gnu s390x" "mips64el-linux-gnuabi64 mips64el"; do
    # shellcheck disable=SC2086 # split "triple qemu-arch"
    set -- $spec
    triple=$1
    qemu=qemu-$2
    if ! command -v "${triple}-gcc" >/dev/null || ! command -v "$qemu" >/dev/null; then
        echo "skipping ${triple}: compiler or ${qemu} not installed"
        continue
    fi
    sysroot=/usr/${triple}
    dir=$(readlink -f "${SCRATCH}")/${triple}
    mkdir -p "$dir"

    # Plenty of exported symbols, so the hash tables have many buckets.
    i=0
    while [ $i -lt 300 ]; do echo "int extra$i(void) { return $i; }"; i=$((i + 1)); done > "$dir/extra.c"
    "${triple}-gcc" -fPIC -shared -o "$dir/libbar.so" "${TOP}/bar.c" "$dir/extra.c"
    # shellcheck disable=SC2016 # $ORIGIN is for the dynamic loader
    "${triple}-gcc" -fPIC -shared -o "$dir/libfoo.so" "${TOP}/foo.c" -L"$dir" -lbar -Wl,-rpath,'$ORIGIN'
    "${triple}-gcc" -fPIE -pie -o "$dir/main-pie" "${TOP}/main.c" -L"$dir" -lfoo -Wl,-rpath-link,"$dir"
    "${triple}-gcc" -fno-pie -no-pie -o "$dir/main-nopie" "${TOP}/main.c" -L"$dir" -lfoo -Wl,-rpath-link,"$dir"

    # A long interpreter path that still leads to the target's loader.
    interp=$("$PATCHELF" --print-interpreter "$dir/main-pie")
    longdir="$dir/$(printf 'i%.0s' $(seq 1 150))"
    mkdir -p "$longdir"
    ln -sf "${sysroot}${interp}" "$longdir/ld.so"
    longrpath="/nonexistent/$(printf 'r%.0s' $(seq 1 300)):$dir"

    # run LABEL BINARY [LD_LIBRARY_PATH]
    run() {
        out=$(cd "$dir" && QEMU_LD_PREFIX="$sysroot" "$qemu" -E LD_LIBRARY_PATH="${3:-}" "./$2" 2>&1) && rc=$? || rc=$?
        if [ "$rc" != 46 ] || [ "$out" != "$expected" ]; then
            fail "${triple} $1: exit $rc: $out"
        fi
        if "${READELF}" -lSdW "$dir/$2" 2>&1 >/dev/null | grep -q .; then
            fail "${triple} $1: readelf complains about $2"
        fi
    }

    for kind in pie nopie; do
        main=main-$kind
        run "$kind unpatched" "$main" "$dir"

        cp "$dir/$main" "$dir/$main.a"
        "$PATCHELF" --set-rpath "$longrpath" --set-interpreter "$longdir/ld.so" "$dir/$main.a"
        run "$kind long rpath and interpreter" "$main.a"

        cp "$dir/$main.a" "$dir/$main.b"
        "$PATCHELF" --add-needed libm.so.6 --add-debug-tag --set-execstack "$dir/$main.b"
        "$PATCHELF" --clear-execstack --shrink-rpath "$dir/$main.b"
        run "$kind needed, debug tag, execstack, shrink" "$main.b"

        cp "$dir/$main.b" "$dir/$main.c"
        "$PATCHELF" --remove-rpath "$dir/$main.c"
        run "$kind removed rpath" "$main.c" "$dir"
    done

    # Renaming rebuilds the hash tables; every other symbol must still resolve.
    # (Not supported on MIPS, where symbol order is tied to the GOT.)
    case "$triple" in
        mips*) ;;
        *)
            i=0
            while [ $i -lt 50 ]; do echo "extra$i renamed_extra$i"; i=$((i + 1)); done > "$dir/map"
            "$PATCHELF" --rename-dynamic-symbols "$dir/map" "$dir/libbar.so"
            run "renamed symbols" main-pie "$dir"
            ;;
    esac

    # Repeated growth must not keep adding segments.
    before=$("${READELF}" -lW "$dir/libfoo.so" | grep -c LOAD)
    n=0
    while [ $n -lt 20 ]; do
        "$PATCHELF" --set-soname "libfoo-$(printf 'n%.0s' $(seq 1 $((n * 10 + 1)))).so" "$dir/libfoo.so"
        n=$((n + 1))
    done
    after=$("${READELF}" -lW "$dir/libfoo.so" | grep -c LOAD)
    if [ "$after" -gt $((before + 1)) ]; then
        fail "${triple}: repeated updates added $((after - before)) segments"
    fi
    run "repeated updates" main-pie "$dir"

    tested=$((tested + 1))
done

if [ "$tested" = 0 ]; then
    echo "no cross toolchains available"
    exit 77
fi
test "$failed" = 0
