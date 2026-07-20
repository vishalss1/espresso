//! Package manager for Espresso OS.
//! SD-local packages in /pkg/<name>/ with manifest.txt + main.espr + data/.
//! SHA-256 verified installs, atomic write (delete + write pattern).

use crate::drivers::sd;

pub const MAX_PACKAGES: usize = 16;
pub const MAX_PKG_NAME: usize = 24;
pub const MAX_HASH_LEN: usize = 64; // hex string

#[derive(Copy, Clone)]
pub struct Package {
    pub name: [u8; MAX_PKG_NAME],
    pub name_len: u8,
    pub version: [u8; 16],
    pub version_len: u8,
    pub installed: bool,
}

pub static mut PACKAGES: [Package; MAX_PACKAGES] = [Package {
    name: [0; MAX_PKG_NAME],
    name_len: 0,
    version: [0; 16],
    version_len: 0,
    installed: false,
}; MAX_PACKAGES];

pub static mut PACKAGE_COUNT: usize = 0;

fn copy_str(dst: &mut [u8], src: &str) -> usize {
    let len = core::cmp::min(dst.len(), src.len());
    dst[..len].copy_from_slice(&src.as_bytes()[..len]);
    len
}

// ── SHA-256 (no_std, ~80 lines) ─────────────────────────────────────────────

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256 {
    pub fn new() -> Self {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g; g = f; f = e; e = d.wrapping_add(t1);
            d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut i = 0;
        self.total_len += data.len() as u64;
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            let take = core::cmp::min(need, data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            i = take;
            if self.buf_len == 64 {
                let block = self.buf;
                self.process_block(&block);
                self.buf_len = 0;
            }
        }
        while i + 64 <= data.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[i..i + 64]);
            self.process_block(&block);
            i += 64;
        }
        if i < data.len() {
            let rem = data.len() - i;
            self.buf[..rem].copy_from_slice(&data[i..]);
            self.buf_len = rem;
        }
    }

    pub fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len * 8;
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            while self.buf_len < 64 { self.buf_len += 1; }
            let block = self.buf;
            self.process_block(&block);
            self.buf_len = 0;
            self.buf = [0u8; 64];
        }
        while self.buf_len < 56 { self.buf_len += 1; }
        self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buf;
        self.process_block(&block);
        let mut out = [0u8; 32];
        for i in 0..8 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.state[i].to_be_bytes());
        }
        out
    }
}

pub fn sha256_hex(data: &[u8], out: &mut [u8; 64]) {
    let hash = Sha256::new();
    let digest = {
        let mut h = hash;
        h.update(data);
        h.finalize()
    };
    const HEX: [u8; 16] = *b"0123456789abcdef";
    for i in 0..32 {
        out[i * 2] = HEX[(digest[i] >> 4) as usize];
        out[i * 2 + 1] = HEX[(digest[i] & 0x0F) as usize];
    }
}

// ── Manifest parser ─────────────────────────────────────────────────────────
// manifest.txt format:
//   name=<name>
//   version=<version>
//   sha256=<64 hex chars>
//   capabilities=<caps>
//   description=<text>

pub struct Manifest {
    pub name: [u8; MAX_PKG_NAME],
    pub name_len: usize,
    pub version: [u8; 16],
    pub version_len: usize,
    pub sha256: [u8; 64],
    pub sha256_valid: bool,
}

impl Manifest {
    pub fn empty() -> Self {
        Manifest {
            name: [0; MAX_PKG_NAME],
            name_len: 0,
            version: [0; 16],
            version_len: 0,
            sha256: [0; 64],
            sha256_valid: false,
        }
    }
}

fn trim(s: &str) -> &str {
    let s = s.trim_start();
    let s = s.trim_end();
    s
}

pub fn parse_manifest(buf: &[u8]) -> Manifest {
    let mut m = Manifest::empty();
    if let Ok(text) = core::str::from_utf8(buf) {
        for line in text.lines() {
            let line = trim(line);
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some(val) = line.strip_prefix("name=") {
                let val = trim(val);
                m.name_len = copy_str(&mut m.name, val);
            } else if let Some(val) = line.strip_prefix("version=") {
                let val = trim(val);
                m.version_len = copy_str(&mut m.version, val);
            } else if let Some(val) = line.strip_prefix("sha256=") {
                let val = trim(val);
                if val.len() == 64 {
                    m.sha256.copy_from_slice(val.as_bytes());
                    m.sha256_valid = true;
                }
            }
        }
    }
    m
}

// ── Package operations ──────────────────────────────────────────────────────

const MANIFEST_BUF_SIZE: usize = 1024;
const ESPR_BUF_SIZE: usize = 4096;

#[link_section = ".large_bss"]
static mut MBUF: [u8; MANIFEST_BUF_SIZE] = [0u8; MANIFEST_BUF_SIZE];

#[link_section = ".large_bss"]
static mut EBUF: [u8; ESPR_BUF_SIZE] = [0u8; ESPR_BUF_SIZE];

fn make_pkg_path(name: &str, suffix: &str, out: &mut [u8]) -> usize {
    let prefix = b"/pkg/";
    let mut pos = 0;
    for &b in prefix { out[pos] = b; pos += 1; }
    for &b in name.as_bytes() { out[pos] = b; pos += 1; }
    if !suffix.is_empty() {
        out[pos] = b'/'; pos += 1;
        for &b in suffix.as_bytes() { out[pos] = b; pos += 1; }
    }
    pos
}

pub fn pkg_list() {
    unsafe {
        if PACKAGE_COUNT == 0 {
            crate::tty::write_str_both("(no packages installed)\n");
            return;
        }
        crate::tty::write_str_both("NAME                 VERSION    STATUS\n");
        for i in 0..PACKAGE_COUNT {
            let pkg = &PACKAGES[i];
            let name = core::str::from_utf8(&pkg.name[..pkg.name_len as usize]).unwrap_or("?");
            let ver = core::str::from_utf8(&pkg.version[..pkg.version_len as usize]).unwrap_or("?");
            let status = if pkg.installed { "installed" } else { "cached" };
            crate::tty::write_str_both(name);
            let mut pad = name.len();
            while pad < 21 { crate::tty::write_both(b' '); pad += 1; }
            crate::tty::write_str_both(ver);
            pad = ver.len();
            while pad < 11 { crate::tty::write_both(b' '); pad += 1; }
            crate::tty::write_str_both(status);
            crate::tty::write_both(b'\n');
        }
    }
}

pub fn pkg_install(name: &str) -> Result<(), &'static str> {
    let mut manifest_path = [0u8; 64];
    let manifest_len = make_pkg_path(name, "manifest.txt", &mut manifest_path);
    let manifest_str = core::str::from_utf8(&manifest_path[..manifest_len]).map_err(|_| "ERR_BAD_PATH")?;

    unsafe {
        let mlen = sd::read_file_to_buf(manifest_str, &mut MBUF).map_err(|e| e)?;

        if mlen == 0 { return Err("ERR_EMPTY_MANIFEST"); }

        let manifest = parse_manifest(&MBUF[..mlen]);
        if manifest.name_len == 0 { return Err("ERR_NO_NAME_IN_MANIFEST"); }

        let mut espr_path = [0u8; 64];
        let espr_len = make_pkg_path(name, "main.espr", &mut espr_path);
        let espr_str = core::str::from_utf8(&espr_path[..espr_len]).map_err(|_| "ERR_BAD_PATH")?;

        let elen = sd::read_file_to_buf(espr_str, &mut EBUF).map_err(|e| e)?;

        if elen == 0 { return Err("ERR_EMPTY_ESPR"); }

        if manifest.sha256_valid {
            let mut computed = [0u8; 64];
            sha256_hex(&EBUF[..elen], &mut computed);
            if computed != manifest.sha256 {
                crate::tty::write_str_both("  WARN: hash mismatch!\n  expected: ");
                let expected_str = core::str::from_utf8(&manifest.sha256).unwrap_or("?");
                crate::tty::write_str_both(expected_str);
                crate::tty::write_str_both("\n  computed: ");
                let computed_str = core::str::from_utf8(&computed).unwrap_or("?");
                crate::tty::write_str_both(computed_str);
                crate::tty::write_str_both("\n");
                return Err("ERR_HASH_MISMATCH");
            }
            crate::tty::write_str_both("  hash OK\n");
        } else {
            crate::tty::write_str_both("  WARN: no sha256 in manifest, skipping verify\n");
        }

        let mut found = false;
        for i in 0..PACKAGE_COUNT {
            if PACKAGES[i].name_len as usize == name.len()
                && &PACKAGES[i].name[..name.len()] == name.as_bytes()
            {
                PACKAGES[i].version_len = manifest.version_len as u8;
                PACKAGES[i].version[..manifest.version_len].copy_from_slice(&manifest.version[..manifest.version_len]);
                PACKAGES[i].installed = true;
                found = true;
                break;
            }
        }
        if !found && PACKAGE_COUNT < MAX_PACKAGES {
            let pkg = &mut PACKAGES[PACKAGE_COUNT];
            pkg.name_len = name.len() as u8;
            pkg.name[..name.len()].copy_from_slice(name.as_bytes());
            pkg.version_len = manifest.version_len as u8;
            pkg.version[..manifest.version_len].copy_from_slice(&manifest.version[..manifest.version_len]);
            pkg.installed = true;
            PACKAGE_COUNT += 1;
        }
    }

    crate::tty::write_str_both("  installed '");
    crate::tty::write_str_both(name);
    crate::tty::write_str_both("'\n");
    Ok(())
}

pub fn pkg_update(name: &str) -> Result<(), &'static str> {
    crate::tty::write_str_both("  updating '");
    crate::tty::write_str_both(name);
    crate::tty::write_str_both("'...\n");
    pkg_install(name)
}

pub fn pkg_verify(name: &str) -> Result<(), &'static str> {
    let mut manifest_path = [0u8; 64];
    let manifest_len = make_pkg_path(name, "manifest.txt", &mut manifest_path);
    let manifest_str = core::str::from_utf8(&manifest_path[..manifest_len]).map_err(|_| "ERR_BAD_PATH")?;

    unsafe {
        let mlen = sd::read_file_to_buf(manifest_str, &mut MBUF).map_err(|e| e)?;

        if mlen == 0 { return Err("ERR_EMPTY_MANIFEST"); }
        let manifest = parse_manifest(&MBUF[..mlen]);

        let mut espr_path = [0u8; 64];
        let espr_len = make_pkg_path(name, "main.espr", &mut espr_path);
        let espr_str = core::str::from_utf8(&espr_path[..espr_len]).map_err(|_| "ERR_BAD_PATH")?;

        let elen = sd::read_file_to_buf(espr_str, &mut EBUF).map_err(|e| e)?;

        if !manifest.sha256_valid {
            return Err("ERR_NO_SHA256");
        }

        let mut computed = [0u8; 64];
        sha256_hex(&EBUF[..elen], &mut computed);

        if computed == manifest.sha256 {
            crate::tty::write_str_both("  verify OK\n");
            Ok(())
        } else {
            crate::tty::write_str_both("  ERR: hash mismatch\n");
            Err("ERR_HASH_MISMATCH")
        }
    }
}

pub fn pkg_uninstall(name: &str) -> Result<(), &'static str> {
    unsafe {
        for i in 0..PACKAGE_COUNT {
            if PACKAGES[i].name_len as usize == name.len()
                && &PACKAGES[i].name[..name.len()] == name.as_bytes()
            {
                PACKAGES[i].installed = false;
                crate::tty::write_str_both("  uninstalled '");
                crate::tty::write_str_both(name);
                crate::tty::write_str_both("'\n");
                return Ok(());
            }
        }
    }
    Err("ERR_NOT_FOUND")
}

pub fn find_package(name: &str) -> Option<usize> {
    unsafe {
        for i in 0..PACKAGE_COUNT {
            if PACKAGES[i].name_len as usize == name.len()
                && &PACKAGES[i].name[..name.len()] == name.as_bytes()
            {
                return Some(i);
            }
        }
    }
    None
}

pub fn get_package_path(name: &str, buf: &mut [u8]) -> Result<usize, &'static str> {
    let len = make_pkg_path(name, "main.espr", buf);
    Ok(len)
}
