//! C and C++ DPI sources named among the design files (IEEE 1800-2023 §35).
//!
//! A testbench's file list often names its DPI-C code next to the RTL
//! (`add.c`, `model.cpp`). Such a file is not SystemVerilog: it is compiled
//! with the host C or C++ compiler, linked with any object files and static
//! archives named the same way into one shared library, and loaded exactly
//! like a `--dpi-lib` library. A shared library named among the files
//! (`.so`, `.dylib`, `.dll`) is loaded as it is.
//!
//! The compiler is the one the DPI export trampoline uses: `$CC` for C and
//! `$CXX` for C++ (either may carry leading arguments, `ccache gcc`), else
//! `clang`/`clang++`, else `cc`/`c++` when Clang is not on `PATH`. xezim's
//! own `svdpi.h`, `vpi_user.h`, `sv_vpi_user.h` and `veriuser.h` are on the
//! include path. `-CFLAGS`/`-LDFLAGS` add compiler and linker arguments.
//!
//! The library is cached by content under `<cache>/xezim/dpi`: the key covers
//! each source's PREPROCESSED text (so an edited header misses), the
//! compiler commands, the object files' bytes and the link flags. A hit
//! skips the compiler entirely, apart from the preprocessing pass.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What a design-file argument is when it is not HDL source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForeignKind {
    /// A C source: `.c`.
    C,
    /// A C++ source: `.cc`, `.cpp`, `.cxx`, `.c++` or `.C`.
    Cxx,
    /// An object file or static archive (`.o`, `.a`), linked into the library.
    Object,
    /// A shared library (`.so`, `.dylib`, `.dll`), loaded as `--dpi-lib` is.
    SharedLibrary,
}

/// The kind of `path` by its extension, or `None` for HDL source.
pub fn foreign_kind(path: &str) -> Option<ForeignKind> {
    let ext = Path::new(path).extension()?.to_str()?;
    Some(match ext {
        "c" => ForeignKind::C,
        "C" | "cc" | "cpp" | "cxx" | "c++" => ForeignKind::Cxx,
        "o" | "a" => ForeignKind::Object,
        "so" | "dylib" | "dll" => ForeignKind::SharedLibrary,
        _ => return None,
    })
}

/// The non-HDL files of a run, in the order they were named.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ForeignInputs {
    /// C and C++ sources.
    pub sources: Vec<(String, ForeignKind)>,
    /// Object files and static archives.
    pub objects: Vec<String>,
    /// Shared libraries.
    pub shared_libraries: Vec<String>,
}

impl ForeignInputs {
    /// Move every C/C++ source, object, archive and shared library out of
    /// `files`, which keeps its HDL sources in order.
    pub fn take_from(files: &mut Vec<String>) -> Self {
        let mut out = Self::default();
        files.retain(|f| match foreign_kind(f) {
            None => true,
            Some(k @ (ForeignKind::C | ForeignKind::Cxx)) => {
                out.sources.push((f.clone(), k));
                false
            }
            Some(ForeignKind::Object) => {
                out.objects.push(f.clone());
                false
            }
            Some(ForeignKind::SharedLibrary) => {
                out.shared_libraries.push(f.clone());
                false
            }
        });
        out
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.objects.is_empty() && self.shared_libraries.is_empty()
    }

    /// Every file, for `--dump-files-list`.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.sources
            .iter()
            .map(|(f, _)| f.as_str())
            .chain(self.objects.iter().map(String::as_str))
            .chain(self.shared_libraries.iter().map(String::as_str))
    }
}

/// A path `dlopen` takes as a file rather than a name to search the library
/// path for: one without a `/` (`lib.so`) is made relative to `.`.
pub fn loadable_path(path: &str) -> String {
    if path.contains('/') {
        path.to_string()
    } else {
        format!("./{}", path)
    }
}

/// Where built libraries are kept: `xezim/dpi` under `$XEZIM_CACHE_DIR`,
/// `$XDG_CACHE_HOME` or `~/.cache`, as the native-code cache does.
pub fn default_cache_dir() -> PathBuf {
    let set = |v: &str| std::env::var_os(v).filter(|p| !p.is_empty());
    set("XEZIM_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| set("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| set("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("xezim")
        .join("dpi")
}

/// The headers DPI-C code includes, as xezim ships them in `include/`.
const HEADERS: [(&str, &str); 4] = [
    ("svdpi.h", include_str!("../include/svdpi.h")),
    ("sv_vpi_user.h", include_str!("../include/sv_vpi_user.h")),
    ("vpi_user.h", include_str!("../include/vpi_user.h")),
    ("veriuser.h", include_str!("../include/veriuser.h")),
];

/// The compiler command for one language: `$CC`/`$CXX` split on whitespace,
/// else Clang, else the system's default `cc`/`c++`.
fn compiler(var: &str, clang: &str, fallback: &str) -> Vec<OsString> {
    if let Ok(v) = std::env::var(var) {
        let words: Vec<OsString> = v.split_whitespace().map(Into::into).collect();
        if !words.is_empty() {
            return words;
        }
    }
    let on_path = std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| d.join(clang).is_file())
    });
    vec![if on_path { clang } else { fallback }.into()]
}

fn display(cmd: &[OsString]) -> String {
    cmd.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run `cmd`; its diagnostics go straight to the terminal. An error names
/// what was being done.
fn run(cmd: &[OsString], what: &str) -> Result<(), String> {
    match Command::new(&cmd[0]).args(&cmd[1..]).status() {
        Ok(st) if st.success() => Ok(()),
        Ok(st) => Err(format!(
            "{} failed (exit {}): {}",
            what,
            st.code().map_or("signal".to_string(), |c| c.to_string()),
            display(cmd)
        )),
        Err(e) => Err(format!(
            "{}: cannot run `{}` ({}); set $CC/$CXX or put clang or cc on PATH",
            what,
            cmd[0].to_string_lossy(),
            e
        )),
    }
}

/// Write the shipped headers into a directory named by their content, once.
fn header_dir(cache: &Path) -> Result<PathBuf, String> {
    let mut h = crate::CacheHash::new();
    for (name, text) in HEADERS {
        h.text(name);
        h.text(text);
    }
    let dir = cache.join(format!("include-{}", &h.finish()[..16]));
    if HEADERS.iter().all(|(n, _)| dir.join(n).is_file()) {
        return Ok(dir);
    }
    // Written beside it and renamed, so a concurrent run never reads a
    // half-written header.
    let tmp = cache.join(format!("include-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&tmp)?;
        for (name, text) in HEADERS {
            std::fs::write(tmp.join(name), text)?;
        }
        Ok(())
    };
    write().map_err(|e| format!("cannot write DPI headers under {}: {}", cache.display(), e))?;
    if std::fs::rename(&tmp, &dir).is_err() {
        // Another run put them there first.
        let _ = std::fs::remove_dir_all(&tmp);
    }
    Ok(dir)
}

/// Compile the C/C++ sources of `inputs` and link them with its objects and
/// archives into one shared library under `cache`, or reuse the library an
/// earlier run built from the same inputs. `None` when there is nothing to
/// build. `cflags` go to every compile after xezim's own arguments, so they
/// can override them (`-O0 -g`); `ldflags` go last on the link.
pub fn build_library(
    inputs: &ForeignInputs,
    cflags: &[String],
    ldflags: &[String],
    cache: &Path,
) -> Result<Option<PathBuf>, String> {
    if inputs.sources.is_empty() && inputs.objects.is_empty() {
        return Ok(None);
    }
    std::fs::create_dir_all(cache)
        .map_err(|e| format!("cannot create the DPI cache {}: {}", cache.display(), e))?;
    let include = header_dir(cache)?;
    let cc = compiler("CC", "clang", "cc");
    let cxx = compiler("CXX", "clang++", "c++");
    let flags = |lang: ForeignKind| -> Vec<OsString> {
        let mut cmd = if lang == ForeignKind::Cxx { cxx.clone() } else { cc.clone() };
        cmd.extend(["-fPIC", "-O2", "-I"].map(OsString::from));
        cmd.push(include.clone().into());
        cmd.extend(cflags.iter().map(OsString::from));
        cmd
    };

    // The key: what each source preprocesses to under its exact compile
    // command, the objects' bytes, and the link.
    let mut key = crate::CacheHash::new();
    key.text("dpi-sources-1");
    key.text(std::env::consts::OS);
    key.text(std::env::consts::ARCH);
    for (src, lang) in &inputs.sources {
        let mut pp = flags(*lang);
        pp.extend(["-E".into(), src.into()]);
        let out = Command::new(&pp[0])
            .args(&pp[1..])
            .output()
            .map_err(|e| {
                format!(
                    "DPI source '{}': cannot run `{}` ({}); set $CC/$CXX or put clang or cc \
                     on PATH",
                    src,
                    pp[0].to_string_lossy(),
                    e
                )
            })?;
        if !out.status.success() {
            return Err(format!(
                "DPI source '{}' failed to preprocess: {}\n{}",
                src,
                display(&pp),
                String::from_utf8_lossy(&out.stderr).trim_end()
            ));
        }
        key.text(&display(&flags(*lang)));
        key.bytes(&out.stdout);
    }
    for obj in &inputs.objects {
        let bytes = std::fs::read(obj)
            .map_err(|e| format!("cannot read DPI object '{}': {}", obj, e))?;
        key.text(obj);
        key.bytes(&bytes);
    }
    let any_cxx = inputs.sources.iter().any(|(_, k)| *k == ForeignKind::Cxx);
    let mut link = if any_cxx { cxx.clone() } else { cc.clone() };
    link.extend(["-shared", "-fPIC"].map(OsString::from));
    // Exported SV subroutines and the svdpi/VPI calls resolve when the
    // library is loaded into xezim, not when it is linked.
    if cfg!(target_os = "macos") {
        link.extend(["-undefined", "dynamic_lookup"].map(OsString::from));
    }
    key.text(&display(&link));
    for f in ldflags {
        key.text(f);
    }
    let key = key.finish();
    let lib = cache.join(format!("dpi-{}.so", &key[..24]));
    if lib.is_file() {
        return Ok(Some(lib));
    }

    let build = cache.join(format!("build-{}-{}", &key[..24], std::process::id()));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| format!("cannot create {}: {}", build.display(), e))?;
    let result = (|| -> Result<(), String> {
        let mut objs: Vec<OsString> = Vec::new();
        for (i, (src, lang)) in inputs.sources.iter().enumerate() {
            let obj = build.join(format!("{}.o", i));
            let mut cmd = flags(*lang);
            cmd.extend(["-c".into(), src.into(), "-o".into(), obj.clone().into()]);
            run(&cmd, &format!("compiling DPI source '{}'", src))?;
            objs.push(obj.into());
        }
        let tmp = build.join("lib.so");
        link.extend(["-o".into(), tmp.clone().into()]);
        link.extend(objs);
        for obj in &inputs.objects {
            // Every member of an archive: nothing in the library references
            // them, the simulator looks them up by name.
            if obj.ends_with(".a") {
                if cfg!(target_os = "macos") {
                    link.push(format!("-Wl,-force_load,{}", obj).into());
                } else {
                    link.extend([
                        "-Wl,--whole-archive".into(),
                        obj.into(),
                        "-Wl,--no-whole-archive".into(),
                    ]);
                }
            } else {
                link.push(obj.into());
            }
        }
        link.extend(ldflags.iter().map(OsString::from));
        run(&link, "linking the DPI library")?;
        std::fs::rename(&tmp, &lib)
            .map_err(|e| format!("cannot move the DPI library to {}: {}", lib.display(), e))
    })();
    let _ = std::fs::remove_dir_all(&build);
    result.map(|()| Some(lib))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(foreign_kind("a/add.c"), Some(ForeignKind::C));
        for f in ["m.cc", "m.cpp", "m.cxx", "m.c++", "m.C"] {
            assert_eq!(foreign_kind(f), Some(ForeignKind::Cxx), "{f}");
        }
        assert_eq!(foreign_kind("x.o"), Some(ForeignKind::Object));
        assert_eq!(foreign_kind("libx.a"), Some(ForeignKind::Object));
        assert_eq!(foreign_kind("libx.so"), Some(ForeignKind::SharedLibrary));
        for f in ["t.sv", "t.v", "t.svh", "t.vh", "noext", "t.sv.bak"] {
            assert_eq!(foreign_kind(f), None, "{f}");
        }
    }

    #[test]
    fn take_from_keeps_hdl_order() {
        let mut files: Vec<String> = ["a.sv", "add.c", "b.v", "m.cpp", "x.o", "l.so", "c.svh"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let f = ForeignInputs::take_from(&mut files);
        assert_eq!(files, ["a.sv", "b.v", "c.svh"]);
        assert_eq!(
            f.sources,
            [
                ("add.c".to_string(), ForeignKind::C),
                ("m.cpp".to_string(), ForeignKind::Cxx)
            ]
        );
        assert_eq!(f.objects, ["x.o"]);
        assert_eq!(f.shared_libraries, ["l.so"]);
    }

    #[test]
    fn bare_library_names_load_as_files() {
        assert_eq!(loadable_path("lib.so"), "./lib.so");
        assert_eq!(loadable_path("d/lib.so"), "d/lib.so");
        assert_eq!(loadable_path("/abs/lib.so"), "/abs/lib.so");
    }
}
