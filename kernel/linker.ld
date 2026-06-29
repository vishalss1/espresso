// Linker script for ESP32 - stage 1 bootloader region
// Kernel image will occupy Flash 0x010000 - 0x020000
// RAM regions marked for kernel allocation

MEMORY {
    IRAM : ORIGIN = 0x40370000, LENGTH = 0x10000;
    DRAM : ORIGIN = 0x3FFB0000, LENGTH = 0x200000;
}

SECTIONS {
    .kernel.text : {
        *(.kernel.text)
        *(.text)
    } > IRAM

    .kernel.data : {
        *(.kernel.data)
        *(.data)
    } > DRAM AT > IRAM

    .kernel.bss : {
        *(.kernel.bss)
        *(.bss)
    } > DRAM
}