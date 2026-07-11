# Espresso OS — Xtensa LX6 context switch
# switch(TaskControlBlock *current, TaskControlBlock *next)
# a2 = current TCB pointer, a3 = next TCB pointer
#
# TCB layout (offsets in bytes):
#   0x00  pid         (u32)
#   0x04  state       (u32)
#   0x08  stack_base  (u32)
#   0x0C  stack_size  (u32)
#   0x10  sp          (u32)
#   0x14  entry       (u32)
#   0x18  caps        (u32)

    .text
    .global switch
    .align 4

switch:
    entry   a1, 16
    s32i    a1,  a2, 0x10
    l32i    a1,  a3, 0x10
    retw
