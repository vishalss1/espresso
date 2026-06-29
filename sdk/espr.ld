[[espr.ld]]

# Linker script for Espresso .espr format executable
# 
# Memory regions:
# - KERNEL_BASE: 0x3FF00000 - Kernel text area
# - EXEC_POOL_START: 0x3FFBC800 - Program execution pool (328KB)
# - EXEC_POOL_END: 0x40000000 - End of exec pool
# 
# Sections:
# - .text: Machine code (executable)
# - .data: Initialized program data
# - .bss: Zero-initialized data (size from header)

ENTRY(_start)

MEMORY {
    EXEC : ORIGIN = EXEC_POOL_START, LENGTH = (EXEC_POOL_END - EXEC_POOL_START)
}

SECTIONS {
    .text : {
        *(.text)
        *(.text*)
    } > EXEC

    .data : {
        *(.data)
        *(.data*)
    } > EXEC

    .bss : {
        *(.bss)
        *(.bss*)
        *(COMMON)
    } > EXEC
}