//! Issue #307: C/C++ DPI sources named among the design files (IEEE 1800-2023
//! §35.5). A file list that names `add.c` next to `t.sv` used to have the C
//! file parsed as SystemVerilog. Such a source is now compiled with the host
//! C/C++ compiler, with xezim's `svdpi.h` on the include path, into a cached
//! shared library loaded like `--dpi-lib`; `.o`/`.a` files are linked into
//! it and a `.so` is loaded as it is.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xezim_dpi_csrc_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cache")).unwrap();
    d
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

/// Run xezim in `dir` with its DPI cache under `dir/cache`.
fn run(dir: &Path, args: &[&str]) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--no-cache"])
        .args(args)
        .current_dir(dir)
        .env("XEZIM_CACHE_DIR", dir.join("cache"))
        .output()
        .expect("failed to run xezim");
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out, text)
}

/// The libraries built into the scratch cache.
fn built_libraries(dir: &Path) -> usize {
    std::fs::read_dir(dir.join("cache").join("xezim").join("dpi"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".so"))
                .count()
        })
        .unwrap_or(0)
}

fn cc() -> String {
    std::env::var("CC").unwrap_or_else(|_| "clang".to_string())
}

const ADD_SV: &str = r#"
module t;
  import "DPI-C" function int c_add(input int a, input int b);
  initial begin $display("sum=%0d", c_add(2, 3)); $finish; end
endmodule
"#;

/// The issue's case: `add.c` before `t.sv` in a `-f` list.
#[test]
fn c_source_in_a_file_list_is_compiled_and_loaded() {
    let d = scratch("flist");
    write(&d, "add.c", "int c_add(int a, int b) { return a + b; }\n");
    write(&d, "t.sv", ADD_SV);
    write(&d, "files.f", "add.c\nt.sv\n");
    let (out, text) = run(&d, &["-f", "files.f", "-s", "t"]);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("sum=5"), "{text}");
    assert!(!text.contains("Parse error"), "{text}");
    assert_eq!(built_libraries(&d), 1, "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A C++ source on the command line: compiled and linked as C++ (it uses the
/// standard library), with `svdpi.h` found without an `-I`, `-CFLAGS`
/// reaching the compiler, and a call back into an exported SV function.
#[test]
fn cxx_source_with_svdpi_cflags_and_an_export() {
    let d = scratch("cxx");
    write(
        &d,
        "model.cpp",
        r#"#include "svdpi.h"
#include <vector>
extern "C" int sv_twice(int a);
extern "C" int c_scale(int a) {
  std::vector<int> v{a, SCALE};
  return v[0] * v[1];
}
extern "C" svBit c_odd(int a) { return (svBit)(sv_twice(a) / 2 & 1); }
"#,
    );
    write(
        &d,
        "t.sv",
        r#"
module t;
  import "DPI-C" function int c_scale(input int a);
  import "DPI-C" function bit c_odd(input int a);
  export "DPI-C" function sv_twice;
  function int sv_twice(input int a); return 2 * a; endfunction
  initial begin $display("scaled=%0d odd=%0d", c_scale(7), c_odd(7)); $finish; end
endmodule
"#,
    );
    let (out, text) = run(&d, &["-CFLAGS", "-DSCALE=3 -Wall", "model.cpp", "t.sv"]);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("scaled=21 odd=1"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The library is reused while the sources and the headers they include are
/// unchanged, and rebuilt when either changes.
#[test]
fn library_is_cached_by_preprocessed_source() {
    let d = scratch("cache");
    write(&d, "k.h", "#define K 0\n");
    write(&d, "add.c", "#include \"k.h\"\nint c_add(int a, int b) { return a + b + K; }\n");
    write(&d, "t.sv", ADD_SV);
    let (_, text) = run(&d, &["add.c", "t.sv"]);
    assert!(text.contains("sum=5"), "{text}");
    let (_, text) = run(&d, &["add.c", "t.sv"]);
    assert!(text.contains("sum=5"), "{text}");
    assert_eq!(built_libraries(&d), 1, "an unchanged build was not reused");
    write(&d, "k.h", "#define K 100\n");
    let (_, text) = run(&d, &["add.c", "t.sv"]);
    assert!(text.contains("sum=105"), "an edited header was not rebuilt: {text}");
    assert_eq!(built_libraries(&d), 2);
    let _ = std::fs::remove_dir_all(&d);
}

/// An object file and a static archive are linked into the library, and a
/// shared library in the list is loaded as `--dpi-lib` is.
#[test]
fn objects_archives_and_shared_libraries_in_a_file_list() {
    let d = scratch("objs");
    write(&d, "one.c", "int c_one(void) { return 1; }\n");
    write(&d, "two.c", "int c_two(void) { return 2; }\n");
    write(&d, "four.c", "int c_four(void) { return 4; }\n");
    write(&d, "eight.c", "int c_eight(void) { return 8; }\n");
    let ok = |c: &mut Command| c.current_dir(&d).status().is_ok_and(|s| s.success());
    assert!(ok(Command::new(cc()).args(["-fPIC", "-c", "two.c", "-o", "two.o"])));
    assert!(ok(Command::new(cc()).args(["-fPIC", "-c", "four.c", "-o", "four.o"])));
    let ar = std::env::var("AR").unwrap_or_else(|_| "ar".to_string());
    assert!(ok(Command::new(ar).args(["rcs", "libfour.a", "four.o"])));
    assert!(ok(Command::new(cc()).args(["-shared", "-fPIC", "eight.c", "-o", "libeight.so"])));
    write(
        &d,
        "t.sv",
        r#"
module t;
  import "DPI-C" function int c_one();
  import "DPI-C" function int c_two();
  import "DPI-C" function int c_four();
  import "DPI-C" function int c_eight();
  initial begin $display("total=%0d", c_one() + c_two() + c_four() + c_eight()); $finish; end
endmodule
"#,
    );
    write(&d, "files.f", "one.c\ntwo.o\nlibfour.a\nlibeight.so\nt.sv\n");
    let (out, text) = run(&d, &["--error-exit", "-f", "files.f"]);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("total=15"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A source that does not compile stops the run before it starts.
#[test]
fn compile_error_stops_the_run() {
    let d = scratch("error");
    write(&d, "add.c", "int c_add(int a, int b) { return a + ; }\n");
    write(&d, "t.sv", ADD_SV);
    let (out, text) = run(&d, &["add.c", "t.sv"]);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("DPI source 'add.c'"), "{text}");
    assert!(!text.contains("sum="), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// `--dump-files-list` shows the DPI files apart from the HDL sources.
#[test]
fn files_list_shows_dpi_sources() {
    let d = scratch("list");
    write(&d, "add.c", "int c_add(int a, int b) { return a + b; }\n");
    write(&d, "t.sv", ADD_SV);
    write(&d, "files.f", "add.c\nt.sv\n");
    let (_, text) = run(&d, &["--dump-files-list", "-f", "files.f"]);
    assert!(text.contains("1 source file(s)"), "{text}");
    assert!(text.contains("--- DPI C/C++ file(s): 1 ---"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}
