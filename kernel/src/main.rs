#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

use core::panic::PanicInfo;

extern "C" {
    static mut _bss_start: u32;
    static mut _bss_end: u32;
    static mut _data_start: u32;
    static mut _data_end: u32;
    static _data_load: u32;
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

const fn to_array_32(s: &[u8]) -> [u8; 32] {
    let mut arr = [0; 32];
    let mut i = 0;
    while i < s.len() && i < 32 {
        arr[i] = s[i];
        i += 1;
    }
    arr
}

/// esp_app_desc_t — required at DROM base + 0x20 so the bootloader can find it.
/// Placed in .rodata_desc which is the very first thing in the DROM segment.
#[repr(C)]
pub struct esp_app_desc_t {
    pub magic_word: u32,
    pub secure_version: u32,
    pub reserv1: [u32; 2],
    pub version: [u8; 32],
    pub project_name: [u8; 32],
    pub time: [u8; 16],
    pub date: [u8; 16],
    pub idf_ver: [u8; 32],
    pub app_elf_sha256: [u8; 32],
    pub min_efuse_blk_rev_full: u16,
    pub max_efuse_blk_rev_full: u16,
    pub mmu_page_size: u8,
    pub spi_flash_mode: u8,
    pub reserv3: [u8; 2],
    pub reserv2: [u32; 18],
}

#[used]
#[no_mangle]
#[link_section = ".rodata_desc"]
pub static esp_app_desc: esp_app_desc_t = esp_app_desc_t {
    magic_word: 0xABCD5432,
    secure_version: 0,
    reserv1: [0; 2],
    version: to_array_32(b"0.1.0"),
    project_name: to_array_32(b"espresso"),
    time: [0; 16],
    date: [0; 16],
    idf_ver: to_array_32(b"v5.5.1"),
    app_elf_sha256: [0; 32],
    min_efuse_blk_rev_full: 0,
    max_efuse_blk_rev_full: 0,
    mmu_page_size: 0,
    spi_flash_mode: 0,
    reserv3: [0; 2],
    reserv2: [0; 18],
};

/// Kernel entry point — runs after bootloader maps IROM via MMU.
/// UART0 TX FIFO is at 0x3FF40000. We write bytes directly; no UART init
/// needed for basic TX at the reset baud rate (115200).
#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        let uart0_fifo = 0x3FF40000 as *mut u32;
        let msg = b"Espresso\r\n";
        loop {
            for &b in msg {
                core::ptr::write_volatile(uart0_fifo, b as u32);
            }
            for _ in 0..10_000_000u32 {
                core::arch::asm!("nop");
            }
        }
    }
}