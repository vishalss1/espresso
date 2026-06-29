#!todo!("Assembler stub for xtensa-esp32-none-elf - assembles switch.S into object file")

use std::env;
use std::fs::File;
use std::io::Write;

fn main() {
    // TODO: Assemble switch.S using xtensa-esp32-elf-gcc
    let out_dir = env::var("OUT_DIR").unwrap();
    let switch_path = "src/kernel/asm/switch.S";
    let target_obj = format!("{}/switch.o", out_dir);
    
    // Placeholder: just create an empty file for now
    let mut file = File::create(target_obj).unwrap();
    writeln!(file, "// TODO: switch.S should be assembled here").unwrap();
    
    println!("Compiling asm/switch.S for Xtensa target");
}