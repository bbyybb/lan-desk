use sha2::{Digest, Sha256};
use std::path::Path;
use tracing::{error, info};

const SEALED_FILES: &[(&str, &str)] = &[
    (
        "docs/wechat_pay.jpg",
        "686b9d5bba59d6831580984cb93804543f346d943f2baf4a94216fd13438f1e6",
    ),
    (
        "docs/alipay.jpg",
        "510155042b703d23f7eeabc04496097a7cc13772c5712c8d0716bab5962172dd",
    ),
    (
        "docs/bmc_qr.png",
        "bfd20ef305007c3dacf30dde49ce8f0fe4d7ac3ffcc86ac1f83bc1e75cccfcd6",
    ),
    (
        "README.md",
        "4f42e0176ddb410c24509600852624b4399c854849d23c681e16489942176cce",
    ),
];

const _MK: &[&str] = &[
    "\u{767d}\u{767d}LOVE\u{5c39}\u{5c39}",
    "LANDESK-bbloveyy-2026",
    "bbyybb",
    "buymeacoffee.com/bbyybb",
    "sponsors/bbyybb",
];

pub fn check_sealed_files(base_dir: &Path) -> Result<(), Vec<String>> {
    let mut t = Vec::new();

    if SEALED_FILES.len() < 2 {
        t.push("table corrupted".to_string());
        return Err(t);
    }

    for (rp, eh) in SEALED_FILES {
        let fp = base_dir.join(rp);
        if !fp.exists() {
            t.push(format!("{} (missing)", rp));
            continue;
        }
        let data = match std::fs::read(&fp) {
            Ok(d) => d,
            Err(e) => {
                t.push(format!("{} ({})", rp, e));
                continue;
            }
        };
        let ah = format!("{:x}", Sha256::digest(&data));
        if !crate::ct_eq_bytes(ah.as_bytes(), eh.as_bytes()) {
            t.push(format!("{} (modified)", rp));
        }
    }

    if t.is_empty() {
        info!("sealed files OK ({})", SEALED_FILES.len());
        Ok(())
    } else {
        error!("sealed check FAILED: {:?}", t);
        Err(t)
    }
}

pub fn check_markers_in_text(text: &str) -> bool {
    for m in _MK {
        if !text.contains(m) {
            return false;
        }
    }
    true
}

pub fn is_sealed_path(path: &Path, base_dir: &Path) -> bool {
    let n = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    for (rp, _) in SEALED_FILES {
        if let Ok(s) = base_dir.join(rp).canonicalize() {
            if n == s {
                return true;
            }
        }
    }
    false
}
