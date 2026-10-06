//! Windows (MSVC): gives `mtg-duelstone.exe` its icon, which Explorer, the taskbar and shortcuts show.
//!
//! Why by hand: the icon is the only resource, a `.res` file is simple enough to write here, and the
//! MSVC linker takes one as an input, so no resource compiler or extra crate is needed.

use std::path::PathBuf;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    let ico = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/windows/mtg-duelstone.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    let target = |k: &str| std::env::var(k).unwrap_or_default();
    if target("CARGO_CFG_TARGET_OS") != "windows" || target("CARGO_CFG_TARGET_ENV") != "msvc" {
        return;
    }
    let ico = std::fs::read(&ico).expect("read packaging/windows/mtg-duelstone.ico");
    let res = PathBuf::from(target("OUT_DIR")).join("icon.res");
    std::fs::write(&res, icon_res(&ico)).expect("write icon.res");
    println!("cargo:rustc-link-arg-bin=mtg-duelstone={}", res.display());
}

/// Every image of `ico` as an RT_ICON (ids 1, 2, …) plus the RT_GROUP_ICON (id 1) that lists them.
fn icon_res(ico: &[u8]) -> Vec<u8> {
    let u32_at = |i: usize| u32::from_le_bytes(ico[i..i + 4].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes([ico[4], ico[5]]);
    // A .res file starts with an empty entry: no data, ordinal type 0 and name 0.
    let mut out = vec![0, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0];
    out.extend([0; 16]);
    // GRPICONDIR: reserved, type 1 (icon), count
    let mut group = vec![0, 0, 1, 0];
    group.extend(count.to_le_bytes());
    for i in 0..count {
        let e = 6 + 16 * i as usize;
        let (size, offset) = (u32_at(e + 8), u32_at(e + 12));
        entry(&mut out, RT_ICON, i + 1, 0x1010, &ico[offset..offset + size]);
        // GRPICONDIRENTRY: the file's ICONDIRENTRY with the resource id in place of the offset
        group.extend(&ico[e..e + 12]);
        group.extend((i + 1).to_le_bytes());
    }
    entry(&mut out, RT_GROUP_ICON, 1, 0x1030, &group);
    out
}

/// One resource: a 32-byte header with an ordinal type and name (US English), then the data,
/// padded to 4 bytes.
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes());
    for v in [0xFFFF, kind, 0xFFFF, id] {
        out.extend(v.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(flags.to_le_bytes());
    out.extend(0x0409u16.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    out.resize(out.len().next_multiple_of(4), 0);
}
