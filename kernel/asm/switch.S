# Espresso OS — Xtensa LX6 context switch + SYSCALL exception handler
#
# switch(TaskControlBlock *current, TaskControlBlock *next)
#   a2 = current TCB pointer, a3 = next TCB pointer
#
# Call0 ABI context switch: save a0 (return address) + sp on current stack,
# load sp + a0 from next stack, jump to new a0.
#
# TCB layout (offsets in bytes):
#   0x00  pid         (u32)
#   0x04  state       (u32)
#   0x08  stack_base  (u32)
#   0x0C  stack_size  (u32)
#   0x10  sp          (u32)
#   0x14  entry       (u32)
#   0x18  caps        (u32)
#   0x1C  ps          (u32)   — processor state (PS register), saved/restored
#                           across context switches so that PS.EXCM does not
#                           leak from one task to another.

    .text
    .global switch
    .align 4

switch:
    addi    a1, a1, -32
    s32i    a0, a1, 0
    s32i    a12, a1, 4
    s32i    a13, a1, 8
    s32i    a14, a1, 12
    s32i    a15, a1, 16
    s32i    a1, a2, 0x10
    rsr     a4, PS
    s32i    a4, a2, 0x1C

    l32i    a1, a3, 0x10
    l32i    a4, a3, 0x1C
    wsr     a4, PS

    l32i    a0, a1, 0
    l32i    a12, a1, 4
    l32i    a13, a1, 8
    l32i    a14, a1, 12
    l32i    a15, a1, 16
    addi    a1, a1, 32
    rsync
    ret.n

# ── Vector table (.vec section — NO literals allowed here) ───────────────────
#
# This section must contain ONLY the 16 vector slots and the trampoline.
# NO `movi` with large constants — those go in .text to avoid the assembler
# placing a literal pool at the start of this section (which shifts
# espresso_vecbase and breaks VECBASE alignment).
#
# The linker script merges .vec before .text in the IROM output section,
# so espresso_vecbase ends up at the IROM base (0x400D0000).

    .section .vec, "ax", @progbits
    .align 4
    .global espresso_vecbase
espresso_vecbase:

.macro panic_slot
    break  0, 0
    j      _panic_addr
    .align 64
.endm

    panic_slot  # 0x000 WindowOverflow4
    panic_slot  # 0x040 WindowUnderflow4
    panic_slot  # 0x080 WindowOverflow8
    panic_slot  # 0x0C0 WindowUnderflow8
    panic_slot  # 0x100 WindowOverflow12
    panic_slot  # 0x140 WindowUnderflow12
    panic_slot  # 0x180 Level2Interrupt
    panic_slot  # 0x1C0 Level3Interrupt
    panic_slot  # 0x200 Level4Interrupt
    panic_slot  # 0x240 Level5Interrupt
    panic_slot  # 0x280 DebugException
    panic_slot  # 0x2C0 NMIException
    # Slot 12 (0x300) — KernelException: jump to .text handler that prints 'K'
    .align 64
    j       _kernel_exc_entry
    .align 64

    # Slot 13 (0x340) — UserException trampoline
    .align 64
    j       _syscall_handler

    # Slot 14 (0x380) — unused
    .align 64
    panic_slot
    # Slot 15 (0x3C0) — DoubleException
    .align 64
    panic_slot

_panic_addr:
    j      espresso_vecbase

# ── KernelException handler — prints 'K' then halts ─────────────────────────
    .text
    .align 4
_kernel_exc_entry:
    movi    a8, 0x3FF40000
    movi    a10, 'K'
    s32i    a10, a8, 0
_kernel_exc_loop:
    j       _kernel_exc_loop

# ── Full SYSCALL handler (.text section — literals go here) ──────────────────

    .text
    .align 4
    .global _syscall_entry
_syscall_entry:
_syscall_handler:
    wsr    a0, excsave1
    addi   a1, a1, -128

    addi   a7, a1, 124
    addi   a7, a7, 4
    s32i   a7, a1, 4
    s32i   a3, a1, 12
    s32i   a4, a1, 16
    s32i   a5, a1, 20

    # Save EPC1 and EXCSAVE1 to stack frame to protect across context switches
    rsr    a7, epc1
    s32i   a7, a1, 24
    rsr    a7, excsave1
    s32i   a7, a1, 28

    rsr    a6, EXCCAUSE
    movi   a7, 1
    bne    a6, a7, _not_syscall

    s32i   a2, a1, 32
    l32i   a6, a1, 12
    s32i   a6, a1, 36
    l32i   a6, a1, 16
    s32i   a6, a1, 40
    l32i   a6, a1, 20
    s32i   a6, a1, 44
    movi   a6, 0
    s32i   a6, a1, 48

    movi   a6, CURRENT_TASK
    l32i   a6, a6, 0
    s32i   a6, a1, 52

    addi   a2, a1, 32
    movi   a6, syscall_dispatch_c
    callx0 a6

    s32i   a2, a1, 8

    # Restore EPC1 and EXCSAVE1 from stack frame and advance EPC1 by 3
    l32i   a6, a1, 24
    addi   a6, a6, 3
    wsr    a6, epc1
    l32i   a6, a1, 28
    wsr    a6, excsave1

    l32i   a5, a1, 20
    l32i   a4, a1, 16
    l32i   a3, a1, 12
    l32i   a2, a1, 8
    l32i   a1, a1, 4

    rsr    a0, excsave1
    rfe

_not_syscall:
    # a6 = EXCCAUSE  a8..a12 are free scratch registers
    # Print:  "\r\n!EXC exccause=XX pc=XXXXXXXX\r\n"  then hang

    movi   a8, 0x3FF40000        # UART0 FIFO

    movi   a10, 13;  s32i a10, a8, 0  # CR
    movi   a10, 10;  s32i a10, a8, 0  # LF
    movi   a10, '!'; s32i a10, a8, 0
    movi   a10, 'E'; s32i a10, a8, 0
    movi   a10, 'X'; s32i a10, a8, 0
    movi   a10, 'C'; s32i a10, a8, 0
    movi   a10, '='; s32i a10, a8, 0

    # --- Print EXCCAUSE (a6) as 2 hex nibbles ---
    srli   a9,  a6, 4
    extui  a9,  a9, 0, 4
    movi   a11, 10
    bgeu   a9,  a11, .Lne_hi1
    addi   a10, a9, 48          # '0'
    j      .Lne_hp1
.Lne_hi1:
    addi   a10, a9, 55          # 'A'-10
.Lne_hp1:
    s32i   a10, a8, 0

    extui  a9,  a6, 0, 4
    movi   a11, 10
    bgeu   a9,  a11, .Lne_hi2
    addi   a10, a9, 48
    j      .Lne_hp2
.Lne_hi2:
    addi   a10, a9, 55
.Lne_hp2:
    s32i   a10, a8, 0

    # --- Print " PC=" ---
    movi   a10, 32;  s32i a10, a8, 0  # space
    movi   a10, 'P'; s32i a10, a8, 0
    movi   a10, 'C'; s32i a10, a8, 0
    movi   a10, '='; s32i a10, a8, 0

    # --- Print EPC1 as 8 hex nibbles ---
    rsr    a9, epc1
    movi   a12, 10               # constant 10, reused for each nibble

    srli   a11, a9, 28; extui a11, a11, 0, 4
    bgeu   a11, a12, .Ln7h; addi a10, a11, 48; j .Ln7p
.Ln7h: addi a10, a11, 55
.Ln7p: s32i a10, a8, 0

    srli   a11, a9, 24; extui a11, a11, 0, 4
    bgeu   a11, a12, .Ln6h; addi a10, a11, 48; j .Ln6p
.Ln6h: addi a10, a11, 55
.Ln6p: s32i a10, a8, 0

    srli   a11, a9, 20; extui a11, a11, 0, 4
    bgeu   a11, a12, .Ln5h; addi a10, a11, 48; j .Ln5p
.Ln5h: addi a10, a11, 55
.Ln5p: s32i a10, a8, 0

    srli   a11, a9, 16; extui a11, a11, 0, 4
    bgeu   a11, a12, .Ln4h; addi a10, a11, 48; j .Ln4p
.Ln4h: addi a10, a11, 55
.Ln4p: s32i a10, a8, 0

    srli   a11, a9, 12; extui a11, a11, 0, 4
    bgeu   a11, a12, .Ln3h; addi a10, a11, 48; j .Ln3p
.Ln3h: addi a10, a11, 55
.Ln3p: s32i a10, a8, 0

    srli   a11, a9, 8;  extui a11, a11, 0, 4
    bgeu   a11, a12, .Ln2h; addi a10, a11, 48; j .Ln2p
.Ln2h: addi a10, a11, 55
.Ln2p: s32i a10, a8, 0

    srli   a11, a9, 4;  extui a11, a11, 0, 4
    bgeu   a11, a12, .Ln1h; addi a10, a11, 48; j .Ln1p
.Ln1h: addi a10, a11, 55
.Ln1p: s32i a10, a8, 0

    extui  a11, a9, 0, 4
    bgeu   a11, a12, .Ln0h; addi a10, a11, 48; j .Ln0p
.Ln0h: addi a10, a11, 55
.Ln0p: s32i a10, a8, 0

    movi   a10, 13; s32i a10, a8, 0   # CR
    movi   a10, 10; s32i a10, a8, 0   # LF

    # --- Print " VA=" (EXCVADDR = faulting load/store address) ---
    movi   a10, ' '; s32i a10, a8, 0
    movi   a10, 'V'; s32i a10, a8, 0
    movi   a10, 'A'; s32i a10, a8, 0
    movi   a10, '='; s32i a10, a8, 0

    rsr    a9, EXCVADDR             # faulting virtual address

    srli   a11, a9, 28; extui a11, a11, 0, 4
    bgeu   a11, a12, .Lva7h; addi a10, a11, 48; j .Lva7p
.Lva7h: addi a10, a11, 55
.Lva7p: s32i a10, a8, 0

    srli   a11, a9, 24; extui a11, a11, 0, 4
    bgeu   a11, a12, .Lva6h; addi a10, a11, 48; j .Lva6p
.Lva6h: addi a10, a11, 55
.Lva6p: s32i a10, a8, 0

    srli   a11, a9, 20; extui a11, a11, 0, 4
    bgeu   a11, a12, .Lva5h; addi a10, a11, 48; j .Lva5p
.Lva5h: addi a10, a11, 55
.Lva5p: s32i a10, a8, 0

    srli   a11, a9, 16; extui a11, a11, 0, 4
    bgeu   a11, a12, .Lva4h; addi a10, a11, 48; j .Lva4p
.Lva4h: addi a10, a11, 55
.Lva4p: s32i a10, a8, 0

    srli   a11, a9, 12; extui a11, a11, 0, 4
    bgeu   a11, a12, .Lva3h; addi a10, a11, 48; j .Lva3p
.Lva3h: addi a10, a11, 55
.Lva3p: s32i a10, a8, 0

    srli   a11, a9, 8;  extui a11, a11, 0, 4
    bgeu   a11, a12, .Lva2h; addi a10, a11, 48; j .Lva2p
.Lva2h: addi a10, a11, 55
.Lva2p: s32i a10, a8, 0

    srli   a11, a9, 4;  extui a11, a11, 0, 4
    bgeu   a11, a12, .Lva1h; addi a10, a11, 48; j .Lva1p
.Lva1h: addi a10, a11, 55
.Lva1p: s32i a10, a8, 0

    extui  a11, a9, 0, 4
    bgeu   a11, a12, .Lva0h; addi a10, a11, 48; j .Lva0p
.Lva0h: addi a10, a11, 55
.Lva0p: s32i a10, a8, 0

    movi   a10, 13; s32i a10, a8, 0
    movi   a10, 10; s32i a10, a8, 0

    # Kill the faulting task and switch to the next alive task.
    # Never returns.
    movi   a10, kill_and_switch
    callx0 a10

_loop_hang:
    j      _loop_hang

# ── setup_vecbase: install the vector table at boot ──────────────────────────
    .global setup_vecbase
    .align 4
setup_vecbase:
    movi   a2, espresso_vecbase
    wsr    a2, VECBASE
    rsync
    ret.n
