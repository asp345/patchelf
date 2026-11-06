# Builds the test programs and runs every test script against $(PATCHELF).
#
#   make -C tests check                 # uses ../target/release/patchelf
#   make -C tests check PATCHELF=/path/to/patchelf
#   make -C tests check TESTS="set-rpath.sh soname.sh"

PATCHELF ?= $(abspath ../target/release/patchelf)
CC ?= cc
READELF ?= readelf
OBJDUMP ?= objdump
OBJCOPY ?= objcopy
STRIP ?= strip
NM ?= nm
STRINGS ?= strings
JOBS ?= $(shell nproc 2>/dev/null || echo 4)

export PATCHELF READELF OBJDUMP OBJCOPY STRIP NM STRINGS
export PATCHELF_DEBUG = 1
export srcdir = .
# Keep the Nix compiler wrappers from adding run paths of their own.
export NIX_DONT_SET_RPATH = 1
export NIX_LDFLAGS =

NO_RPATH_ARCHES = alpha amd64 armel armhf hurd-i386 i386 ia64 kfreebsd-amd64 \
	kfreebsd-i386 mips mipsel powerpc s390 sh4 sparc
NO_RPATH_TESTS = $(NO_RPATH_ARCHES:%=no-rpath-%.sh)

TESTS ?= $(filter-out no-rpath-prebuild.sh $(NO_RPATH_TESTS),$(sort $(wildcard *.sh))) $(NO_RPATH_TESTS)

LIBS_BUILT = libfoo.so libfoo-scoped.so libbar.so libbar-scoped.so libsimple.so \
	libsimple-execstack.so libbuildid.so libtoomanystrtab.so phdr-corruption.so \
	pht-collision.so libcustom-init.so libmany-syms.so liboveralign.so \
	libshared-rpath.so libbig-dynstr.so
PROGRAMS = simple simple-pie simple-execstack main main-no-pie main-no-pie-rpath \
	large-page main-emit-relocs pad-to-page main-scoped big-dynstr too-many-strtab \
	no-rpath contiguous-note-sections many-syms-main many-syms-no-pie \
	notes-after-interp libbig-dynstr.debug $(LIBS_BUILT)
GENERATED = big-dynstr.c many-syms.c many-syms-main.c

LOCAL = -Wl,--disable-new-dtags -Wl,-rpath-link=. -L.
SHARED = -Wl,--disable-new-dtags -shared -L. -fpic

.PHONY: all check clean
all: $(PROGRAMS) $(NO_RPATH_TESTS)

check: all
	@./run-tests $(JOBS) $(TESTS)

no-rpath-%.sh:
	ln -sf no-rpath-prebuild.sh $@

# ---- generated sources ----
big-dynstr.c: main.c
	cat $< > $@
	i=1; while [ $$i -le 2000 ]; do echo "void f$$i(void) { };"; i=$$(($$i + 1)); done >> $@
many-syms.c:
	i=1; while [ $$i -le 2000 ]; do echo "void f$$i() {};"; i=$$(($$i + 1)); done > $@
many-syms-main.c:
	echo "int main() {" > $@
	i=1; while [ $$i -le 2000 ]; do echo "void f$$i(); f$$i();"; i=$$(($$i + 1)); done >> $@
	echo "}" >> $@

# ---- shared libraries ----
libbar.so: bar.c
	$(CC) $(SHARED) -o $@ $< -Wl,-rpath,`pwd`/no-such-path
libbar-scoped.so: bar.c
	$(CC) $(SHARED) -o $@ $<
libfoo.so: foo.c libbar.so
	$(CC) $(SHARED) -o $@ $< -lbar
libfoo-scoped.so: foo.c libbar-scoped.so
	$(CC) $(SHARED) -o $@ $< -lbar-scoped
libsimple.so: simple.c
	$(CC) $(SHARED) -Wl,-z,noexecstack -o $@ $<
libsimple-execstack.so: simple.c
	$(CC) $(SHARED) -Wl,-z,execstack -o $@ $<
libbuildid.so: simple.c
	$(CC) $(SHARED) -Wl,--build-id -o $@ $<
libtoomanystrtab.so: too-many-strtab.c too-many-strtab2.s
	$(CC) $(SHARED) -o $@ $^
libcustom-init.so: custom-init.c
	$(CC) $(SHARED) -Wl,-init,my_init -o $@ $<
liboveralign.so: simple.c
	$(CC) $(SHARED) -Wl,-z,max-page-size=0x10000 -o $@ $<
libshared-rpath.so: shared-rpath.c
	$(CC) $(SHARED) -Wl,-rpath=a_symbol_name -o $@ $<
libmany-syms.so: many-syms.c
	$(CC) $(SHARED) -o $@ $<
libbig-dynstr.so: big-dynstr.c
	$(CC) -fPIC -shared -o $@ $<
libbig-dynstr.debug: libbig-dynstr.so
	$(STRIP) --only-keep-debug $< -o $@
phdr-corruption.so: void.c phdr-corruption.ld
	$(CC) -nostdlib -shared -Wl,-T$(srcdir)/phdr-corruption.ld -o $@ void.c
pht-collision.so: pht-collision.c pht-collision.ld
	$(CC) -nostdlib -shared -Wl,-T$(srcdir)/pht-collision.ld -o $@ pht-collision.c

# ---- programs ----
simple: simple.c
	$(CC) -Wl,-z,noexecstack -o $@ $<
simple-pie: simple.c
	$(CC) -fPIC -pie -o $@ $<
simple-execstack: simple.c
	$(CC) -Wl,-z,execstack -o $@ $<
main: main.c libfoo.so
	$(CC) -fpic $(LOCAL) -o $@ $< -lfoo
main-no-pie: main.c libfoo.so
	$(CC) -fno-pie $(LOCAL) -no-pie -o $@ $< -lfoo
main-no-pie-rpath: main.c libfoo.so
	$(CC) -fno-pie $(LOCAL) -no-pie -Wl,-rpath,/nonexistent -o $@ $< -lfoo
large-page: simple.c
	$(CC) -fno-pie $(LOCAL) -no-pie -Wl,-z,max-page-size=0x200000 -o $@ $<
main-emit-relocs: main.c libfoo.so
	$(CC) $(LOCAL) -Wl,--emit-relocs -o $@ $< -lfoo
pad-to-page: pad-to-page.c
	$(CC) -o $@ $<
main-scoped: main.c libfoo-scoped.so
	$(CC) -fpic $(LOCAL) -o $@ $< -lfoo-scoped
big-dynstr: big-dynstr.c libfoo.so
	$(CC) -fpic $(LOCAL) -o $@ $< -lfoo
too-many-strtab: too-many-strtab.c too-many-strtab2.s
	$(CC) -fpic -o $@ $^
no-rpath: no-rpath.c
	$(CC) -o $@ $<
contiguous-note-sections: contiguous-note-sections.s contiguous-note-sections.ld
	$(CC) -pie -nostdlib -T $(srcdir)/contiguous-note-sections.ld -o $@ contiguous-note-sections.s
many-syms-main: many-syms-main.c libmany-syms.so
	$(CC) -pie -fPIE $(LOCAL) -o $@ $< -lmany-syms
many-syms-no-pie: many-syms-main.c libmany-syms.so
	$(CC) -fno-pie $(LOCAL) -no-pie -Wl,-z,noseparate-code -Wl,--hash-style=both -Wl,-rpath,/nonexistent -o $@ $< -lmany-syms
notes-after-interp: notes-after-interp.c notes-after-interp.ld
	$(CC) -fno-pie $(LOCAL) -no-pie -Wl,-T,$(srcdir)/notes-after-interp.ld -o $@ notes-after-interp.c

clean:
	$(RM) -r scratch
	$(RM) *.log $(PROGRAMS) $(NO_RPATH_TESTS) $(GENERATED)
