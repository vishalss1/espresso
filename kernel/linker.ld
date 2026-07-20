/* Espresso OS linker script — spec-compliant version
 * Memory layout per CLAUDE.md:
 *   IROM @ 0x400D0000 → .text  (flash-cached code, execute-in-place via MMU)
 *   DROM @ 0x3F400000 → .rodata (flash-cached data via MMU)
 *   STATIC_DATA @ 0x3FFB0000 → .data, .bss, .rodata (RAM)
 *
 * The ESP32 bootloader "maps" IROM/DROM segments via the MMU — they are NOT
 * copied to RAM. The bootloader "loads" DRAM/IRAM segments — those ARE copied.
 *
 * espflash aligns DROM to flash offset 0x010000 (64KB boundary) and IROM to
 * flash offset 0x020000 (128KB boundary). It inserts padding segments
 * (vaddr=0x00000000) between real segments to satisfy MMU page alignment.
 * These padding segments are inert — the bootloader skips them.
 */
ENTRY(_start)

MEMORY {
    /* Flash-cache mapped regions (execute/read in place, NOT loaded to RAM) */
    DROM  (r)  : ORIGIN = 0x3F400000, LENGTH = 4M   /* flash data cache window */
    IROM  (rx) : ORIGIN = 0x400D0000, LENGTH = 4M   /* flash code cache window */

    /* Internal SRAM — kernel static allocations per CLAUDE.md */
    STATIC_DATA (rw) : ORIGIN = 0x3FFB0000, LENGTH = 21K
    TASK_STACKS (rw) : ORIGIN = 0x3FFB5400, LENGTH = 10K
    FORTH_DICT  (rw) : ORIGIN = 0x3FFB7C00, LENGTH = 18K
    LARGE_BSS   (rw) : ORIGIN = 0x3FFD0800, LENGTH = 120K
    EXEC_POOL   (rw) : ORIGIN = 0x3FFBC800, LENGTH = 80K

    /* No separate VEC_TABLE region — vector table is in .text (IROM). */
}

/*
 * Program header flags:
 *   PT_LOAD with R--  (4) → DROM  "map" segment (bootloader sets up MMU, no RAM copy)
 *   PT_LOAD with R-X  (5) → IROM  "map" segment (bootloader sets up MMU, no RAM copy)
 *   PT_LOAD with RW-  (6) → DRAM  "load" segment (bootloader copies to RAM)
 */
PHDRS {
    drom_hdr PT_LOAD FLAGS(4);   /* DROM: read-only flash-mapped data */
    irom_hdr PT_LOAD FLAGS(5);   /* IROM: read-execute flash-mapped code */
    data_hdr PT_LOAD FLAGS(6);   /* DRAM: loaded into SRAM by bootloader */
}

SECTIONS {
    /*
     * .rodata_desc MUST be the VERY FIRST thing in the DROM segment.
     * The bootloader finds esp_app_desc at (DROM vaddr + 32).
     * espflash places DROM at flash offset 0x010000, so:
     *   DROM vaddr     = 0x3F400000
     *   esp_app_desc   = 0x3F400020 (+ 32 bytes for image header)
     */
    .rodata : ALIGN(4) {
        /* The app descriptor MUST be at DROM base + 0x20 */
        KEEP(*(.rodata_desc))
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
        . = ALIGN(4);
    } > DROM :drom_hdr

    /*
     * .text in IROM — executed directly from flash via the MMU cache.
     * The entry point _start will be in this region.
     * espflash places IROM at flash offset 0x020000, so:
     *   IROM vaddr = 0x400D0000
     *   _start     = 0x400D0000 + offset_of(_start_in_.text
     */
    .text : ALIGN(4) {
        *(.vec .vec.*)          /* Vector table FIRST (at IROM base, 4KB-aligned) */
        *(.literal .literal.*)
        *(.text .text.*)
        . = ALIGN(4);
    } > IROM :irom_hdr

    /*
     * .data — initialized data, copied from flash (LMA in IROM) to SRAM (VMA) at boot.
     * The linker places data in SRAM (VMA = STATIC_DATA) but stores it in flash
     * after .text (AT > IROM). _start must copy [_data_load .. _data_load+size)
     * to [_data_start .. _data_end) before using any initialized globals.
     */
    .data : {
        . = ALIGN(4);
        _data_start = .;
        *(.data .data.*)
        *(.sdata .sdata.*)
        . = ALIGN(4);
        _data_end = .;
    } > STATIC_DATA AT > IROM :data_hdr

    _data_load = LOADADDR(.data);

    /*
     * .bss — zero-initialized, not stored in flash, zeroed by _start at boot.
     */
    .bss (NOLOAD) : {
        . = ALIGN(4);
        _bss_start = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        . = ALIGN(4);
        _bss_end = .;
    } > STATIC_DATA :NONE

    .large_bss (NOLOAD) : {
        . = ALIGN(4);
        *(.large_bss)
        . = ALIGN(4);
    } > LARGE_BSS

    /* Export region base addresses for Rust kernel code */
    _task_stacks_start = ORIGIN(TASK_STACKS);
    _forth_dict_start  = ORIGIN(FORTH_DICT);
    _exec_pool_start   = ORIGIN(EXEC_POOL);

    /* Discard debug/metadata sections */
    /DISCARD/ : {
        *(.comment)
        *(.xtensa.info)
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.debug_*)
        *(.note.*)
        *(.gnu.*)
    }
}