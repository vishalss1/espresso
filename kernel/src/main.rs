#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

use core::panic::PanicInfo;

pub mod drivers {
    pub mod uart;
    pub mod spi;
    pub mod delay;
    pub mod sd;
}
pub mod shell;

extern "C" {
    static mut _bss_start: u32;
    static mut _bss_end: u32;
    static mut _data_start: u32;
    static mut _data_end: u32;
    static _data_load: u32;
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::println!("!!! KERNEL PANIC !!!");
    crate::println!("{}", info);
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

unsafe fn init_memory() {
    let mut src = &raw const _data_load;
    let mut dest = &raw mut _data_start;
    let end = &raw mut _data_end;
    while dest < end {
        core::ptr::write_volatile(dest, core::ptr::read_volatile(src));
        dest = dest.add(1);
        src = src.add(1);
    }
    let mut dest = &raw mut _bss_start;
    let end = &raw mut _bss_end;
    while dest < end {
        core::ptr::write_volatile(dest, 0);
        dest = dest.add(1);
    }
}

pub unsafe fn wdt_feed() {
    const RTC_CNTL_WDTWPROTECT: *mut u32 = 0x3FF480A4 as *mut u32;
    const RTC_CNTL_WDTFEED: *mut u32 = 0x3FF480A0 as *mut u32;
    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0x50D83AA1);
    core::ptr::write_volatile(RTC_CNTL_WDTFEED, 1);
    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0);
}

unsafe fn disable_wdt() {
    const RTC_CNTL_WDTWPROTECT: *mut u32 = 0x3FF480A4 as *mut u32;
    const RTC_CNTL_WDTCONFIG0:  *mut u32 = 0x3FF48094 as *mut u32;
    const RTC_CNTL_WDTCONFIG1:  *mut u32 = 0x3FF48098 as *mut u32;
    const RTC_CNTL_WDTFEED:     *mut u32 = 0x3FF480A0 as *mut u32;
    const RTC_CNTL_BROWN_OUT:   *mut u32 = 0x3FF4808C as *mut u32;
    const TIMG0_WDTWPROTECT:    *mut u32 = 0x3FF5F064 as *mut u32;
    const TIMG0_WDTCONFIG0:     *mut u32 = 0x3FF5F048 as *mut u32;

    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0x50D83AA1);
    core::ptr::write_volatile(RTC_CNTL_BROWN_OUT, 0);

    core::ptr::write_volatile(RTC_CNTL_WDTCONFIG0, 0);
    core::ptr::write_volatile(RTC_CNTL_WDTCONFIG1, 0xFFFFFFFF);
    core::ptr::write_volatile(RTC_CNTL_WDTFEED, 1);
    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0);

    core::ptr::write_volatile(TIMG0_WDTWPROTECT, 0x50D83AA1);
    core::ptr::write_volatile(TIMG0_WDTCONFIG0, 0);
    core::ptr::write_volatile(TIMG0_WDTWPROTECT, 0);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        init_memory();
        drivers::uart::RawUart::init();
        disable_wdt();

        crate::println!("");
        crate::println!("======================================");
        crate::println!("Espresso OS — Boot Sequence");
        crate::println!("======================================");

        // SPI init (VSPI, software CS on GPIO5)
        crate::println!("[1/2] SPI init (400 kHz, SW CS GPIO5)...");
        drivers::spi::spi_init();

        // SD card init (diagnostic + embedded-sdmmc mount)
        crate::println!("[2/2] SD card init...");
        match drivers::sd::init_fs() {
            Ok(()) => crate::println!("[2/2] SD card mounted OK"),
            Err(e) => crate::println!("[2/2] SD card FAILED: {}", e),
        }

        crate::println!("======================================");
        crate::println!("Boot complete, launching shell.");
        crate::println!("");

        shell::start_shell();

        crate::println!("Shell halted. Idle.");
        loop { core::arch::asm!("nop"); }
    }
}
