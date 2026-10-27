//! IEEE 1800-2023 §13.5.1 / §11.6.1: on the bytecode-compiled tier,
//! `super.m(...)` gives each actual the width of the formal of the method it
//! calls, the parent's `m`, even when the defining class hides `m` with a
//! different formal width. The probe runs 1500x to clear the compile
//! threshold under `XEZIM_FALLBACK_SITES=1`, which proves the body compiled.
//!
//! Fork patch over upstream's compiled super-call widths (1c50e25d), which
//! took the defining class's `m`; drop it when upstream resolves the widths
//! along the parent chain.

use std::path::Path;
use std::process::Command;

#[test]
fn compiled_super_call_takes_the_parent_formal_width() {
    let test_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/classes/compiled_super_hidden_method_width.sv");
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .env("XEZIM_FALLBACK_SITES", "1")
        .arg("--simulate")
        .arg("-s")
        .arg("top")
        .arg(test_file.to_str().unwrap())
        .output()
        .expect("Failed to execute xezim");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("runs-compiled scope=D.probe"),
        "D.probe did not report runs-compiled:\n{combined}"
    );
    assert!(!combined.contains("TAG_FAIL"), "{combined}");
    assert!(combined.contains("TAG_PASS"), "no TAG_PASS:\n{combined}");
}
