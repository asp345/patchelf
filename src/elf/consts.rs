//! ELF constants used by patchelf (see the System V gABI and the GNU extensions).
#![allow(dead_code)]

pub const ELFMAG: &[u8; 4] = b"\x7fELF";
pub const EI_CLASS: usize = 4;
pub const EI_DATA: usize = 5;
pub const EI_VERSION: usize = 6;
pub const EI_OSABI: usize = 7;
pub const ELFCLASS32: u8 = 1;
pub const ELFCLASS64: u8 = 2;
pub const ELFDATA2LSB: u8 = 1;
pub const ELFDATA2MSB: u8 = 2;
pub const EV_CURRENT: u8 = 1;

pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;

pub const EM_SPARC: u16 = 2;
pub const EM_MIPS: u16 = 8;
pub const EM_MIPS_RS3_LE: u16 = 10;
pub const EM_PPC: u16 = 20;
pub const EM_PPC64: u16 = 21;
pub const EM_S390: u16 = 22;
pub const EM_SPARCV9: u16 = 43;
pub const EM_IA_64: u16 = 50;
pub const EM_AARCH64: u16 = 183;
pub const EM_TILEGX: u16 = 191;
pub const EM_LOONGARCH: u16 = 258;
pub const EM_ALPHA: u16 = 0x9026;

pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;
pub const PT_GNU_PROPERTY: u32 = 0x6474_e553;
pub const PT_MIPS_ABIFLAGS: u32 = 0x7000_0003;
pub const PT_GNU_STACK: u32 = 0x6474_e551;
pub const PT_GNU_RELRO: u32 = 0x6474_e552;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_DYNAMIC: u32 = 6;
pub const SHT_NOTE: u32 = 7;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;
pub const SHT_DYNSYM: u32 = 11;
pub const SHT_GROUP: u32 = 17;
pub const SHT_SYMTAB_SHNDX: u32 = 18;

pub const SHF_ALLOC: u64 = 2;
pub const SHF_INFO_LINK: u64 = 0x40;

pub const SHN_UNDEF: u16 = 0;
pub const SHN_LORESERVE: u16 = 0xff00;
pub const SHN_XINDEX: u16 = 0xffff;
pub const PN_XNUM: u16 = 0xffff;

pub const DT_NULL: u64 = 0;
pub const DT_NEEDED: u64 = 1;
pub const DT_PLTRELSZ: u64 = 2;
pub const DT_PLTGOT: u64 = 3;
pub const DT_HASH: u64 = 4;
pub const DT_STRTAB: u64 = 5;
pub const DT_SYMTAB: u64 = 6;
pub const DT_RELA: u64 = 7;
pub const DT_RELASZ: u64 = 8;
pub const DT_RELAENT: u64 = 9;
pub const DT_STRSZ: u64 = 10;
pub const DT_SYMENT: u64 = 11;
pub const DT_INIT: u64 = 12;
pub const DT_FINI: u64 = 13;
pub const DT_SONAME: u64 = 14;
pub const DT_RPATH: u64 = 15;
pub const DT_REL: u64 = 17;
pub const DT_RELSZ: u64 = 18;
pub const DT_RELENT: u64 = 19;
pub const DT_PLTREL: u64 = 20;
pub const DT_DEBUG: u64 = 21;
pub const DT_JMPREL: u64 = 23;
pub const DT_INIT_ARRAY: u64 = 25;
pub const DT_FINI_ARRAY: u64 = 26;
pub const DT_RUNPATH: u64 = 29;
pub const DT_PREINIT_ARRAY: u64 = 32;
pub const DT_SYMTAB_SHNDX: u64 = 34;
pub const DT_RELRSZ: u64 = 35;
pub const DT_RELR: u64 = 36;
pub const DT_GNU_HASH: u64 = 0x6fff_fef5;
pub const DT_CONFIG: u64 = 0x6fff_fefa;
pub const DT_DEPAUDIT: u64 = 0x6fff_fefb;
pub const DT_AUDIT: u64 = 0x6fff_fefc;
pub const DT_VERSYM: u64 = 0x6fff_fff0;
pub const DT_FLAGS_1: u64 = 0x6fff_fffb;
pub const DT_VERDEF: u64 = 0x6fff_fffc;
pub const DT_VERDEFNUM: u64 = 0x6fff_fffd;
pub const DT_VERNEED: u64 = 0x6fff_fffe;
pub const DT_VERNEEDNUM: u64 = 0x6fff_ffff;
pub const DT_AUXILIARY: u64 = 0x7fff_fffd;
pub const DT_FILTER: u64 = 0x7fff_ffff;
pub const DT_MIPS_RLD_MAP_REL: u64 = 0x7000_0035;
pub const DT_MIPS_XHASH: u64 = 0x7000_0036;

pub const DF_1_NODEFLIB: u64 = 0x800;

/// Dynamic tags whose value is an offset into the dynamic string table.
pub const STRING_TAGS: &[u64] =
    &[DT_NEEDED, DT_SONAME, DT_RPATH, DT_RUNPATH, DT_AUXILIARY, DT_FILTER, DT_CONFIG, DT_DEPAUDIT, DT_AUDIT];

/// Dynamic tags whose value is a virtual address.
pub const ADDRESS_TAGS: &[u64] = &[
    DT_PLTGOT,
    DT_HASH,
    DT_STRTAB,
    DT_SYMTAB,
    DT_RELA,
    DT_INIT,
    DT_FINI,
    DT_REL,
    DT_JMPREL,
    DT_INIT_ARRAY,
    DT_FINI_ARRAY,
    DT_PREINIT_ARRAY,
    DT_SYMTAB_SHNDX,
    DT_RELR,
    DT_GNU_HASH,
    DT_VERSYM,
    DT_VERDEF,
    DT_VERNEED,
    DT_MIPS_XHASH,
];
