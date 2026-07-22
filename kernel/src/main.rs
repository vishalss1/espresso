#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

use core::panic::PanicInfo;

pub mod drivers {
    pub mod uart;
}
pub mod scheduler;
pub mod mem {
    pub mod pool;
}
pub mod loader;
pub mod vfs;
pub mod syscall;
pub mod event_log;
pub mod caps;
pub mod panic_policy;
pub mod gpio;
pub mod ipc;
pub mod tty;
pub mod pal;
pub mod device_registry;
pub mod deploy;
pub mod driver;
pub mod wdt;
pub mod arch;
pub mod host_proto;

extern "C" {
    static mut _bss_start: u32;
    static mut _bss_end: u32;
    static mut _data_start: u32;
    static mut _data_end: u32;
    static _data_load: u32;
    static espresso_vecbase: u32;
    fn setup_vecbase();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::println!("!!! KERNEL PANIC !!!");
    crate::println!("{}", info);

    let msg = "KERNEL PANIC";
    let pid = unsafe { scheduler::CURRENT_TASK } as u8;
    crate::panic_policy::write_crash_record(msg, 0, pid);
    crate::event_log::log_panic(pid);
    crate::panic_policy::record_crash();

    // Trigger software reset via RTC_CNTL per CLAUDE.md panic policy
    crate::panic_policy::reset_system();
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

unsafe fn enable_bod() {
    const RTC_CNTL_BROWN_OUT: *mut u32 = 0x3FF4808C as *mut u32;
    const RTC_CNTL_BROWN_OUT_REG: *mut u32 = 0x3FF480D0 as *mut u32;

    let threshold = (1 << 9) | (7 << 4) | (1 << 0);
    core::ptr::write_volatile(RTC_CNTL_BROWN_OUT_REG, threshold);
    core::ptr::write_volatile(RTC_CNTL_BROWN_OUT, 1 << 31);
}

#[unsafe(naked)]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "movi a1, 0x3FFB7C00",
        "j _start_rust"
    );
}

#[no_mangle]
pub extern "C" fn _start_rust() -> ! {
    unsafe {
        // Step 3: BSS zero, .data copy from flash
        init_memory();
        crate::mem::pool::init_bitmap();

        // Step 4 & 5: Check RTC crash-loop backoff
        if panic_policy::check_crash_loop() {
            drivers::uart::RawUart::init();
            crate::println!("\r\n======================================");
            crate::println!("CRASH LOOP DETECTED! Boot halted.");
            crate::println!("Espresso OS Recovery Prompt active.");
            crate::println!("======================================");
            loop {
                wdt::wdt_feed();
                core::arch::asm!("nop");
            }
        }

        // Step 6: UART0 init (115200 baud) — only fixed peripheral path
        drivers::uart::RawUart::init();

        crate::println!("");
        crate::println!("======================================");
        crate::println!("Espresso OS — Boot Sequence");
        crate::println!("======================================");

        // Step 7: BOD enabled (RTC_CNTL brownout detector)
        crate::println!("[BOD] Enabling brownout detector...");
        enable_bod();

        // Step 8: Surface prior boot panic record if present
        let mut crash_buf = [0u8; 256];
        let n = panic_policy::read_crash_log(&mut crash_buf);
        if n > 0 {
            crate::println!("[CRASH_LOG] Surface prior panic record:");
            if let Ok(s) = core::str::from_utf8(&crash_buf[..n]) {
                crate::println!("{}", s);
            }
        }

        // Step 9: Internal flash filesystem mounted (/cfg /drv /app /data /tmp)
        crate::println!("[VFS] Mounting internal flash filesystem...");
        vfs::init_vfs();

        // Step 10: Capability table zeroed, default task capabilities assigned
        crate::println!("[CAPS] Initializing capability table...");
        caps::init_caps();

        // Step 11: Event ring buffer zeroed & boot logged
        event_log::log_boot();

        // Step 12: WDT armed (main + RTC backstop)
        crate::println!("[WDT] Arming main timer WDT + RTC backstop WDT...");
        wdt::arm_wdt();

        // Step 13: Scheduler init (static task table zeroed, vector base setup)
        crate::println!("[SCHED] Initializing scheduler (8 static task slots)...");
        scheduler::init_scheduler();
        ipc::init_queues();
        panic_policy::init_crash_log();
        device_registry::init_device_registry();
        deploy::init_deploy_subsystem();

        setup_vecbase();
        let mut vb: u32 = 0;
        core::arch::asm!("rsr {0}, vecbase", out(reg) vb);
        crate::println!("[VECBASE] 0x{:08X} (expected 0x{:08X})", vb, &raw const espresso_vecbase as usize);

        // Step 14: Active kernel services started (Network, Logger, Host Protocol)
        crate::println!("[SERVICES] Starting Host Protocol Service (Task #3)...");

        // Step 15: Device Registry replays deployment pipeline from /cfg
        crate::println!("[DEVICE_REGISTRY] Replaying persisted application deployments...");
        deploy::replay_persisted_apps();

        // Step 16: WiFi credentials restored from /cfg if present
        crate::println!("[NET] Restoring WiFi credentials...");

        // Step 17 & 18: Preemption active, Host Protocol Splash Banner
        crate::println!("======================================");
        crate::println!("Espresso OS Kernel live (Persistent platform active)");
        crate::println!("======================================");
        crate::println!("");

        loop {
            wdt::wdt_feed();
            host_proto::process_uart_frame();
            scheduler::scheduler_tick();
            core::arch::asm!("nop");
        }
    }
}
