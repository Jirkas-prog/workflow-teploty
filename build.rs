use std::{env, fs, path::PathBuf};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = dir.join("thermal.ico");
    let mut dib = Vec::new();
    for n in [40u32, 32, 64] {
        dib.extend(n.to_le_bytes());
    }
    dib.extend(1u16.to_le_bytes());
    dib.extend(32u16.to_le_bytes());
    for n in [0u32, 4096, 0, 0, 0, 0] {
        dib.extend(n.to_le_bytes());
    }
    for y in (0i32..32).rev() {
        for x in 0i32..32 {
            let mut c = [250u8, 248, 245, 255];
            let bulb = (x - 16).pow(2) + (y - 23).pow(2) <= 49;
            let stem = (12..=20).contains(&x) && (4..=24).contains(&y);
            if bulb || stem {
                c = [98, 110, 22, 255];
            }
            if (14..=18).contains(&x) && (7..=22).contains(&y) {
                c = [218, 238, 241, 255];
            }
            if (x - 16).pow(2) + (y - 23).pow(2) <= 16
                || ((15..=17).contains(&x) && (13..=23).contains(&y))
            {
                c = [76, 181, 211, 255];
            }
            dib.extend(c);
        }
    }
    dib.extend([0u8; 128]);
    let mut ico = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
    ico.extend((dib.len() as u32).to_le_bytes());
    ico.extend(22u32.to_le_bytes());
    ico.extend(dib);
    fs::write(&icon, ico).unwrap();
    winresource::WindowsResource::new()
        .set_icon(icon.to_str().unwrap())
        .set("FileDescription", "WorkFlow Teploty")
        .set("ProductName", "WorkFlow Teploty")
        .set_manifest(r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo><compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application></compatibility></assembly>"#)
        .compile().expect("Windows resources");
}
