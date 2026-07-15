#!/usr/bin/env python3
"""ESPRESSO SDK - ELF to .espr Converter

Converts Xtensa ELF executables to Espresso's relocatable .espr format.

.espr layout:
  0x00  4   magic "ESPR" (0x45535052)
  0x04  4   code size
  0x08  4   data size
  0x0C  4   bss size
  0x10  4   entry point offset (from base)
  0x14  4   relocation table offset
  0x18  4   relocation entry count
  0x1C  4   required stack size
  0x20  ... code segment
  0x??  ... data segment
  0x??  ... relocation table (4 bytes/entry: offset from base to patch)

Usage: emit_espr.py input.elf output.espr [stack_size]
"""

import struct
import sys
import hashlib

def read_u16(data, off):
    return struct.unpack_from("<H", data, off)[0]

def read_u32(data, off):
    return struct.unpack_from("<I", data, off)[0]

def parse_elf32(data):
    """Parse a 32-bit ELF and return sections + entry + relocs."""
    if data[:4] != b'\x7fELF':
        raise ValueError("Not an ELF file")
    if data[4] != 1:
        raise ValueError("Not ELF32")
    if data[5] != 1:
        raise ValueError("Not little-endian")

    entry = read_u32(data, 24)
    phoff = read_u32(data, 28)
    shoff = read_u32(data, 32)
    phentsize = read_u16(data, 42)
    phnum = read_u16(data, 44)
    shentsize = read_u16(data, 46)
    shnum = read_u16(data, 48)
    shstrndx = read_u16(data, 50)

    # Read section headers
    sections = []
    shstrtab_data = None
    for i in range(shnum):
        sh = shoff + i * shentsize
        sh_name = read_u32(data, sh)
        sh_type = read_u32(data, sh + 4)
        sh_flags = read_u32(data, sh + 8)
        sh_addr = read_u32(data, sh + 12)
        sh_offset = read_u32(data, sh + 16)
        sh_size = read_u32(data, sh + 20)
        sections.append({
            'name_off': sh_name,
            'type': sh_type,
            'flags': sh_flags,
            'addr': sh_addr,
            'offset': sh_offset,
            'size': sh_size,
        })

    # Read section name string table
    if shstrndx < len(sections):
        s = sections[shstrndx]
        shstrtab_data = data[s['offset']:s['offset']+s['size']]

    def get_name(idx):
        if shstrtab_data and idx < len(shstrtab_data):
            end = shstrtab_data.index(b'\x00', idx)
            return shstrtab_data[idx:end].decode('ascii', errors='replace')
        return ""

    # Name all sections
    for s in sections:
        s['name'] = get_name(s['name_off'])

    # Extract .text, .data, .bss
    text_sec = None
    data_sec = None
    bss_sec = None
    for s in sections:
        if s['name'] == '.text':
            text_sec = s
        elif s['name'] == '.data':
            data_sec = s
        elif s['name'] == '.bss':
            bss_sec = s

    if text_sec is None:
        raise ValueError("No .text section found")

    code = data[text_sec['offset']:text_sec['offset']+text_sec['size']]
    if data_sec and data_sec['size'] > 0:
        initialized_data = data[data_sec['offset']:data_sec['offset']+data_sec['size']]
    else:
        initialized_data = b''
    bss_size = bss_sec['size'] if bss_sec else 0

    # Scan for relocations: find all 32-bit words in .text and .data
    # that point into the load image (absolute addresses to patch)
    relocs = []
    base = text_sec['addr']  # original linked address
    code_size = len(code)
    data_size = len(initialized_data)
    total_size = code_size + data_size

    def scan_section(sec_data, sec_addr, sec_offset_in_image):
        """Scan a section's data for absolute addresses that need relocation."""
        for i in range(0, len(sec_data) - 3, 4):
            val = struct.unpack_from("<I", sec_data, i)[0]
            # Check if the value looks like an address in the program's address space
            if val >= base and val < base + total_size:
                relocs.append(sec_offset_in_image + i)

    scan_section(data[text_sec['offset']:text_sec['offset']+text_sec['size']],
                 text_sec['addr'], 0)
    if data_sec and data_sec['size'] > 0:
        scan_section(data[data_sec['offset']:data_sec['offset']+data_sec['size']],
                     data_sec['addr'], code_size)

    entry_offset = entry - base

    return {
        'code': code,
        'data': initialized_data,
        'bss_size': bss_size,
        'entry_offset': entry_offset,
        'relocs': relocs,
    }


def emit_espr(info, stack_size=4096):
    """Create .espr binary from parsed ELF info."""
    code_size = len(info['code'])
    data_size = len(info['data'])
    reloc_count = len(info['relocs'])

    reloc_offset = 0x20 + code_size + data_size
    header_size = 0x20

    out = bytearray()
    # Header
    out += struct.pack("<I", 0x45535052)        # magic "ESPR"
    out += struct.pack("<I", code_size)          # code size
    out += struct.pack("<I", data_size)          # data size
    out += struct.pack("<I", info['bss_size'])   # bss size
    out += struct.pack("<I", info['entry_offset'])  # entry point offset
    out += struct.pack("<I", reloc_offset)       # relocation table offset
    out += struct.pack("<I", reloc_count)        # relocation count
    out += struct.pack("<I", stack_size)         # required stack size

    # Code + data
    out += info['code']
    out += info['data']

    # Relocation table
    for r in info['relocs']:
        out += struct.pack("<I", r)

    return bytes(out)


def main():
    if len(sys.argv) < 3:
        print("Usage: emit_espr.py input.elf output.espr [stack_size]")
        sys.exit(1)

    elf_path = sys.argv[1]
    espr_path = sys.argv[2]
    stack_size = int(sys.argv[3]) if len(sys.argv) > 3 else 4096

    with open(elf_path, "rb") as f:
        elf_data = f.read()

    info = parse_elf32(elf_data)
    espr = emit_espr(info, stack_size)

    with open(espr_path, "wb") as f:
        f.write(espr)

    # Print SHA-256 of the .espr for manifest
    sha = hashlib.sha256(espr).hexdigest()
    print(f"OK: {espr_path}")
    print(f"  code={len(info['code'])}B data={len(info['data'])}B bss={info['bss_size']}B")
    print(f"  entry_offset=0x{info['entry_offset']:08X} relocs={len(info['relocs'])}")
    print(f"  total={len(espr)}B")
    print(f"  sha256={sha}")


if __name__ == "__main__":
    main()
