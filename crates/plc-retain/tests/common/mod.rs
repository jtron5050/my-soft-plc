//! Temp-dir helper for retain store tests (no extra crates).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use plc_ir::{IrType, RetainLayout, RetainSymbol};
use plc_retain::RetainStore;

static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(label: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "plc-retain-{}-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            n
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self { path }
    }

    pub fn store(&self) -> RetainStore {
        RetainStore::open(&self.path).expect("open store")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub fn simple_layout() -> RetainLayout {
    RetainLayout::new(
        8,
        vec![
            RetainSymbol::new("flag", IrType::Bool, 0),
            RetainSymbol::new("count", IrType::Dint, 4),
        ],
    )
    .unwrap()
}

pub fn image_with(flag: bool, count: i32) -> Vec<u8> {
    let mut img = vec![0u8; 8];
    img[0] = u8::from(flag);
    img[4..8].copy_from_slice(&count.to_le_bytes());
    img
}

#[allow(dead_code)] // used by corruption.rs
pub fn corrupt_crc(path: &std::path::Path) {
    let mut bytes = std::fs::read(path).expect("read slot");
    assert!(bytes.len() >= 32, "slot too small");
    bytes[28] ^= 0xFF;
    std::fs::write(path, bytes).expect("write corrupt slot");
}
