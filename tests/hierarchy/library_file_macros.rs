//! Issue #310: §22.5.1, §3.12.1: a text macro defined in one `-y`/`-v`
//! library file, directly or through an `include`, is visible in the library
//! files read after it. Library files resolved for the design are read as
//! part of the same compilation, in the order they are resolved; each one
//! used to start from the source files' macros only, so a library module
//! that included the design's headers did not share them with the library
//! modules it instantiates, and those failed to parse.

use std::path::Path;
use std::process::Command;

const MID: &str = "`include \"defs.vh\"\nmodule mid;\n  LEAF l();\nendmodule\n";
const LEAF: &str = "module LEAF;\n  initial $display(\"W=%0d\", `W);\nendmodule\n";

fn write(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Run `top2.v` with the library arguments `lib` in `dir`.
fn run(dir: &Path, lib: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .current_dir(dir)
        .args(["--no-cache", "top2.v"])
        .args(lib)
        .args(["+libext+.v", "+incdir+.", "-s", "top"])
        .output()
        .expect("run xezim");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{lib:?}:\n{text}");
    text
}

#[test]
fn macro_from_one_library_file_reaches_the_next() {
    let dir = std::env::temp_dir().join(format!("xezim_libmacro_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    write(&dir, "defs.vh", "`define W 5\n");
    write(
        &dir,
        "top2.v",
        "module top;\n  mid u();\n  initial #1 $finish;\nendmodule\n",
    );
    // The leaf's file sorts before (`aleaf`) and after (`zleaf`) the file
    // that includes the header, and lives in another `-y` directory.
    write(&dir, "lib2/mid.v", &MID.replace("LEAF", "aleaf"));
    write(&dir, "lib2/aleaf.v", &LEAF.replace("LEAF", "aleaf"));
    write(&dir, "lib3/mid.v", &MID.replace("LEAF", "zleaf"));
    write(&dir, "lib3/zleaf.v", &LEAF.replace("LEAF", "zleaf"));
    write(&dir, "lib4a/aleaf.v", &LEAF.replace("LEAF", "aleaf"));
    write(&dir, "lib4b/mid.v", &MID.replace("LEAF", "aleaf"));
    for lib in [
        &["-y", "lib2"][..],
        &["-y", "lib3"],
        &["-y", "lib4a", "-y", "lib4b"],
        &["-v", "lib2/mid.v", "-v", "lib2/aleaf.v"],
    ] {
        let text = run(&dir, lib);
        assert!(text.contains("W=5"), "{lib:?}:\n{text}");
        assert!(!text.contains("undefined"), "{lib:?}:\n{text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
