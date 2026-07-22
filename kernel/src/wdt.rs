//! Watchdog Timer (WDT) module — main + RTC backstop (CLAUDE.md spec)

const RTC_CNTL_WDTWPROTECT: *mut u32 = 0x3FF480A4 as *mut u32;
const RTC_CNTL_WDTCONFIG0:  *mut u32 = 0x3FF48094 as *mut u32;
const RTC_CNTL_WDTCONFIG1:  *mut u32 = 0x3FF48098 as *mut u32;
const RTC_CNTL_WDTFEED:     *mut u32 = 0x3FF480A0 as *mut u32;

const TIMG0_WDTWPROTECT:    *mut u32 = 0x3FF5F064 as *mut u32;
const TIMG0_WDTCONFIG0:     *mut u32 = 0x3FF5F048 as *mut u32;
const TIMG0_WDTFEED:        *mut u32 = 0x3FF5F060 as *mut u32;

/// Arm both main timer WDT and RTC backstop WDT
pub unsafe fn arm_wdt() {
    // 1. Arm Main TIMG0 Watchdog Timer
    core::ptr::write_volatile(TIMG0_WDTWPROTECT, 0x50D83AA1);
    // Stage 0 system reset after timeout, enable WDT
    core::ptr::write_volatile(TIMG0_WDTCONFIG0, (1 << 31) | (1 << 29) | (3 << 23));
    core::ptr::write_volatile(TIMG0_WDTFEED, 1);
    core::ptr::write_volatile(TIMG0_WDTWPROTECT, 0);

    // 2. Arm RTC Backstop Watchdog Timer with longer timeout window
    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0x50D83AA1);
    core::ptr::write_volatile(RTC_CNTL_WDTCONFIG0, (1 << 31) | (1 << 29));
    core::ptr::write_volatile(RTC_CNTL_WDTCONFIG1, 0x000F4240); // RTC timeout threshold
    core::ptr::write_volatile(RTC_CNTL_WDTFEED, 1u32 << 31);
    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0);
}

/// Feed WDT — ONLY called from tick ISR (sys_wdt_feed / tick ISR)
pub unsafe fn wdt_feed() {
    // Feed TIMG0 WDT
    core::ptr::write_volatile(TIMG0_WDTWPROTECT, 0x50D83AA1);
    core::ptr::write_volatile(TIMG0_WDTFEED, 1);
    core::ptr::write_volatile(TIMG0_WDTWPROTECT, 0);

    // Feed RTC WDT (bit 31 is RTC_CNTL_WDT_FEED)
    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0x50D83AA1);
    core::ptr::write_volatile(RTC_CNTL_WDTFEED, 1u32 << 31);
    core::ptr::write_volatile(RTC_CNTL_WDTWPROTECT, 0);
}
