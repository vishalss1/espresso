pub struct RawDelay;

impl embedded_hal::delay::DelayNs for RawDelay {
    fn delay_ns(&mut self, ns: u32) {
        // Tight loop with assembly nop instruction.
        // Assuming CPU clock frequency is 240MHz.
        // 1 nop execution is 1 cycle = ~4.16 ns.
        // We divide ns by 10 to approximate the number of iterations
        // (accounting for branch/increment loop instructions overhead).
        let loops = ns / 10;
        for _ in 0..loops {
            unsafe {
                core::arch::asm!("nop");
            }
        }
    }
}
