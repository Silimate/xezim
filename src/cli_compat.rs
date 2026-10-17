//! Command-line spellings other simulators use for compile + simulate, mapped
//! onto xezim's own behaviour so an existing command line runs unchanged.
//!
//! Accepted on the command line and inside `-f`/`-F` args files. Spellings
//! that collide with an existing xezim flag keep xezim's meaning; see the
//! notes on `c_takes_file` and in main.rs.
//!
//! Pure parsing lives here (no simulator state), so the `-do` subset is
//! unit-testable: tests/misc/cli_compat_args.rs includes this file directly.

use std::path::{Path, PathBuf};

/// State collected from these spellings that outlives the token that set it.
#[derive(Default)]
pub(crate) struct CompatArgs {
    /// `--compat`: the defaults of commercial simulators (no time cap,
    /// conflicting drivers and implicitly static initializers accepted with
    /// a warning, a failing exit status after `$error`). Applied in main.rs.
    pub compat_mode: bool,
    /// `-g`/`-G` overrides in command-line order: (module named by a
    /// `/top/NAME` path, parameter name, value text, `-G`).
    pub param_overrides: Vec<(Option<String>, String, String, bool)>,
    /// `-do` scripts in order: (command text, where it came from).
    pub do_scripts: Vec<(String, String)>,
    /// Library names from `-work`/`-L`/`-Lf`/`-lib`, so `lib.top` names a top.
    pub libs: Vec<String>,
    lib_warned: bool,
    /// `-sv_root <dir>` / `-sv_lib <name>`: DPI shared libraries.
    pub sv_root: Option<String>,
    pub sv_libs: Vec<String>,
    /// `+cover[=<letters>]`, the last one given (`""` for the bare form).
    pub cover: Option<String>,
    /// `-coverage`.
    pub coverage: bool,
    /// `-coverage <types>`, the last one given: the types as written and
    /// what `coverage_types` makes of them.
    pub coverage_types: Option<(String, (u8, String))>,
    /// `-covworkdir`, `-covscope`, `-covtest`; see `cov_db`.
    pub cov_workdir: Option<String>,
    pub cov_scope: Option<String>,
    pub cov_test: Option<String>,
    /// `-uvm` / `--uvm`: add the UVM library named by `XEZIM_UVM_DIR`
    /// (see src/uvm_dir.rs).
    pub uvm: bool,
    /// `--fst-scope-file` files named inside args files (src/cli_files.rs).
    pub fst_scope_files: Vec<String>,
}

/// The code coverage `+cover[=<letters>]` and `-coverage [<types>]` ask
/// for, as `(kinds, letters xezim does not collect, the spelling)`. The
/// letters are other simulators': `s` statement, `b` branch and `t` toggle
/// map onto xezim's kinds; `c` (condition), `e` (expression), `f` (FSM) and
/// `x` (extended toggle) have no xezim counterpart. The bare forms ask for
/// the default set, `sbceft`: all three of xezim's kinds. `+cover=<letters>`
/// wins over `-coverage <types>` (see `coverage_types`).
pub(crate) fn cover_request(cx: &CompatArgs) -> Option<(u8, String, String)> {
    use xezim::compiler::simulator::{KIND_BRANCH, KIND_STATEMENT, KIND_TOGGLE};
    let all = KIND_STATEMENT | KIND_BRANCH | KIND_TOGGLE;
    let spec = match (cx.cover.as_deref(), &cx.coverage_types) {
        (None, Some((types, (kinds, unsupported)))) => {
            return Some((*kinds, unsupported.clone(), format!("-coverage {}", types)));
        }
        (None, None) if !cx.coverage => return None,
        (None, None) => return Some((all, String::new(), "-coverage".to_string())),
        (Some(""), _) => return Some((all, String::new(), "+cover".to_string())),
        (Some(spec), _) => spec,
    };
    let mut kinds = 0u8;
    let mut unsupported = String::new();
    for c in spec.chars() {
        match c {
            's' => kinds |= KIND_STATEMENT,
            'b' => kinds |= KIND_BRANCH,
            't' => kinds |= KIND_TOGGLE,
            _ if !unsupported.contains(c) => unsupported.push(c),
            _ => {}
        }
    }
    Some((kinds, unsupported, format!("+cover={}", spec)))
}

/// `-coverage <types>`: `all` (or `A`), or a `:`-separated list of `B`
/// (block), `E` (expression), `F` (FSM), `T` (toggle) and `U` (functional),
/// in either case, as `(kinds, letters xezim does not collect)` in
/// `+cover`'s letters. A block is a statement or a branch arm, so `B` is
/// statement and branch coverage. Functional coverage (covergroups and
/// assertions) is always collected, so `U` adds nothing. `None` when `spec`
/// is not such a list: then `-coverage` takes no value.
pub(crate) fn coverage_types(spec: &str) -> Option<(u8, String)> {
    use xezim::compiler::simulator::{KIND_BRANCH, KIND_STATEMENT, KIND_TOGGLE};
    let all = KIND_STATEMENT | KIND_BRANCH | KIND_TOGGLE;
    if spec.eq_ignore_ascii_case("all") {
        return Some((all, String::new()));
    }
    let mut kinds = 0u8;
    let mut unsupported = String::new();
    for t in spec.split(':') {
        match t.to_ascii_lowercase().as_str() {
            "a" => kinds |= all,
            "b" => kinds |= KIND_STATEMENT | KIND_BRANCH,
            "t" => kinds |= KIND_TOGGLE,
            "u" => {}
            l @ ("e" | "f") if !unsupported.contains(l) => unsupported.push_str(l),
            "e" | "f" => {}
            _ => return None,
        }
    }
    Some((kinds, unsupported))
}

/// `-covworkdir <dir>`, `-covscope <name>`, `-covtest <name>`: the results
/// file goes in the test's directory of a coverage work area,
/// `<dir>/<scope>/<test>/xezim_cov.json` (defaults `cov_work`, `scope`,
/// `test`). `None` when none of them was given.
pub(crate) fn cov_db(cx: &CompatArgs) -> Option<PathBuf> {
    if cx.cov_workdir.is_none() && cx.cov_scope.is_none() && cx.cov_test.is_none() {
        return None;
    }
    Some(
        Path::new(cx.cov_workdir.as_deref().unwrap_or("cov_work"))
            .join(cx.cov_scope.as_deref().unwrap_or("scope"))
            .join(cx.cov_test.as_deref().unwrap_or("test"))
            .join("xezim_cov.json"),
    )
}

/// `-sv_lib <name>` loads `<sv_root>/<name>.so`; a name that already carries
/// a shared-library extension is used as given.
pub(crate) fn resolve_sv_libs(cx: &CompatArgs) -> Vec<String> {
    let root = Path::new(cx.sv_root.as_deref().unwrap_or("."));
    cx.sv_libs
        .iter()
        .map(|name| {
            let has_ext = [".so", ".dylib", ".dll"].iter().any(|e| name.ends_with(e));
            let file = if has_ext {
                name.clone()
            } else {
                format!("{}.so", name)
            };
            if Path::new(&file).is_absolute() || cx.sv_root.is_none() {
                file
            } else {
                root.join(file).to_string_lossy().into_owned()
            }
        })
        .collect()
}

/// One of the spellings above at `toks[i]`: apply it and return how many
/// tokens it used, or 0 when `toks[i]` is not one of them. `base` is where
/// a relative source file name is also looked up: the args file's directory,
/// or `.` on the command line.
pub(crate) fn handle_flag(
    toks: &[String],
    i: usize,
    base: &Path,
    cx: &mut CompatArgs,
    plusargs: &mut Vec<String>,
) -> Result<usize, String> {
    let t = toks[i].as_str();
    let value = || {
        toks.get(i + 1)
            .cloned()
            .ok_or_else(|| format!("{} requires an argument", t))
    };
    match t {
        // SystemVerilog is always on, every file already shares one
        // compilation unit (macros and `$unit` declarations carry across
        // files), and there is no separate optimizer, GUI or 32-bit mode.
        "-sv" | "-mfcu" | "-quiet" | "-64" | "-32" | "-nologo" | "-batch" => Ok(1),
        _ if t.starts_with("-mfcu=") || is_pass_through_args(t) => Ok(1),
        // No license to queue for, and no elaborated snapshot to rerun
        // (`-R`): every run compiles the sources it is given.
        "-licqueue" | "-R" => Ok(1),
        // The library and scratch directories of a tool that keeps them:
        // xezim writes neither.
        "-xmlibdirname" | "-cds_implicit_tmpdir" => {
            value()?;
            Ok(2)
        }
        // A cap on the number of errors before the tool gives up: xezim
        // reports compile errors and stops, and never stops a simulation on
        // an error count.
        "-errormax" => {
            value()?;
            Ok(2)
        }
        // `--compat`; see `CompatArgs::compat_mode`.
        "--compat" => {
            cx.compat_mode = true;
            Ok(1)
        }
        // Design visibility for debug: xezim keeps every object visible.
        // Assertions and covergroups are always compiled and evaluated.
        "+acc" | "+fcover" | "-sva" | "-assertdebug" => Ok(1),
        _ if t.starts_with("+acc=") || t.starts_with("+access+") => Ok(1),
        "-access" => {
            value()?;
            Ok(2)
        }
        // Code coverage; see `cover_request`.
        "+cover" => {
            cx.cover = Some(String::new());
            Ok(1)
        }
        _ if t.starts_with("+cover=") => {
            cx.cover = Some(t["+cover=".len()..].to_string());
            Ok(1)
        }
        // `-coverage` alone, or with the types to collect (`-coverage all`,
        // `-coverage B:T`) when the next token is such a list and no file.
        "-coverage" => {
            cx.coverage = true;
            let types = toks
                .get(i + 1)
                .filter(|n| !Path::new(n.as_str()).exists() && !base.join(n.as_str()).exists())
                .and_then(|n| coverage_types(n).map(|k| (n.clone(), k)));
            match types {
                Some(types) => {
                    cx.coverage_types = Some(types);
                    Ok(2)
                }
                None => Ok(1),
            }
        }
        "-covworkdir" => {
            cx.cov_workdir = Some(value()?);
            Ok(2)
        }
        "-covscope" => {
            cx.cov_scope = Some(value()?);
            Ok(2)
        }
        "-covtest" => {
            cx.cov_test = Some(value()?);
            Ok(2)
        }
        // xezim replaces its results file on every run.
        "-covoverwrite" => Ok(1),
        "-uvm" | "--uvm" => {
            cx.uvm = true;
            Ok(1)
        }
        "-wlf" => {
            let f = value()?;
            eprintln!(
                "Warning: -wlf {} is ignored: use --fst (or --wave for $dumpvars) for waveforms",
                f
            );
            Ok(2)
        }
        "-sfcu" => {
            eprintln!(
                "Warning: -sfcu is ignored: xezim compiles all files as one compilation unit \
                 (macros and $unit declarations are visible across files)"
            );
            Ok(1)
        }
        "-sv12compat" | "-sv17compat" => {
            xezim::sv_parser::set_sv2023(false);
            Ok(1)
        }
        "-sv05compat" | "-sv09compat" => {
            xezim::sv_parser::set_sv2023(false);
            eprintln!(
                "Warning: {} parses as IEEE 1800-2017, the oldest grammar xezim has",
                t
            );
            Ok(1)
        }
        // Message-number suppression: the numbers belong to another tool.
        "-suppress" | "-nowarn" => {
            value()?;
            Ok(2)
        }
        // Libraries are not persistent: every run compiles its sources.
        "-work" | "-L" | "-Lf" | "-lib" => {
            let lib = value()?;
            if !cx.lib_warned {
                eprintln!(
                    "Warning: {} {}: xezim compiles every run from source and keeps no \
                     libraries; -work/-L/-lib are ignored",
                    t, lib
                );
                cx.lib_warned = true;
            }
            cx.libs.push(lib);
            Ok(2)
        }
        "-t" => {
            let res = value()?;
            eprintln!(
                "Warning: -t {} is ignored: xezim resolves time at the finest precision \
                 declared in the design",
                res
            );
            Ok(2)
        }
        // `-svseed`/`-seed` (also `=<n>`) are more spellings of `-sv_seed`.
        // Here rather than in main.rs, where the glued `-s<top>` took them
        // for a top named `vseed` or `eed`.
        _ if matches!(t, "-sv_seed" | "-svseed" | "-seed")
            || t.starts_with("-svseed=")
            || t.starts_with("-seed=") =>
        {
            let (flag, seed, used) = match t.split_once('=') {
                Some((flag, seed)) => (flag, seed.to_string(), 1),
                None => (t, value()?, 2),
            };
            if !seed.eq_ignore_ascii_case("random") && seed.parse::<u64>().is_err() {
                return Err(format!(
                    "{} wants a non-negative integer or 'random', got '{}'",
                    flag, seed
                ));
            }
            plusargs.push(format!("+seed={}", seed));
            Ok(used)
        }
        "-sv_root" => {
            cx.sv_root = Some(value()?);
            Ok(2)
        }
        "-sv_lib" => {
            cx.sv_libs.push(value()?);
            Ok(2)
        }
        "-do" => {
            let v = value()?;
            let script = read_do_arg(&v)?;
            cx.do_scripts.push(script);
            Ok(2)
        }
        _ if (t.starts_with("-g") || t.starts_with("-G")) && t.len() > 2 && t.contains('=') => {
            let force = t.starts_with("-G");
            let (name, val) = t[2..].split_once('=').unwrap();
            let parts: Vec<&str> = name
                .trim_start_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .collect();
            match parts.as_slice() {
                [p] => cx
                    .param_overrides
                    .push((None, p.to_string(), val.to_string(), force)),
                [m, p] => cx.param_overrides.push((
                    Some(m.to_string()),
                    p.to_string(),
                    val.to_string(),
                    force,
                )),
                _ => eprintln!(
                    "Warning: {} ignored: only parameters of a top-level module can be \
                     named by path (/<top>/<name>)",
                    t
                ),
            }
            Ok(1)
        }
        _ => Ok(0),
    }
}

/// `-<step>args=<list>`: arguments handed through to a separate optimizer
/// or elaboration step, which xezim does not have.
fn is_pass_through_args(t: &str) -> bool {
    t.split_once('=').is_some_and(|(k, _)| {
        k.len() > "-args".len()
            && k.ends_with("args")
            && k.starts_with('-')
            && !k.starts_with("--")
            && k[1..].chars().all(|c| c.is_ascii_lowercase())
    })
}

/// `-do <arg>`: a macro file when `arg` names one, else the commands inline.
fn read_do_arg(arg: &str) -> Result<(String, String), String> {
    if Path::new(arg).is_file() {
        return std::fs::read_to_string(arg)
            .map(|text| (text, format!("-do {}", arg)))
            .map_err(|e| format!("-do: cannot read '{}': {}", arg, e));
    }
    let one_word = !arg.trim().contains(char::is_whitespace);
    if one_word && (arg.ends_with(".do") || arg.ends_with(".tcl")) {
        return Err(format!("-do: cannot read '{}': no such file", arg));
    }
    Ok((arg.to_string(), "-do".to_string()))
}

/// `-c`: xezim's args-file flag (`-c <file>`, same as `-f`). Other
/// simulators spell "command-line (batch) mode" `-c` with no argument. The
/// args-file form wins whenever the next token names a file or looks like a
/// path, so every command line that worked before keeps its meaning; a bare
/// design-unit name (`top`, `work.top`) or another option after `-c` means
/// the no-argument form.
pub(crate) fn c_takes_file(next: Option<&str>, libs: &[String]) -> bool {
    let Some(n) = next else {
        return false;
    };
    if n.starts_with('-') || n.starts_with('+') {
        return false;
    }
    if Path::new(n).is_file() {
        return true;
    }
    bare_top_name(n, libs).is_none()
}

/// A top-level design unit given as a bare argument: an identifier, or
/// `<lib>.<unit>` for `work` or a library named earlier, that is not an
/// existing file. Anything else stays a source file.
pub(crate) fn bare_top_name(arg: &str, libs: &[String]) -> Option<String> {
    if Path::new(arg).exists() {
        return None;
    }
    if is_identifier(arg) {
        return Some(arg.to_string());
    }
    let (lib, unit) = arg.split_once('.')?;
    let known_lib = lib == "work" || libs.iter().any(|l| l == lib);
    (known_lib && is_identifier(unit)).then(|| unit.to_string())
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// How long a `-do` script asks the one simulation run to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DoRun {
    /// No `run` before the script ended or quit: load the design only.
    Load,
    /// `run <time>` (summed over several): stop after this many ns.
    For(u64),
    /// `run -all`: until `$finish` or no events remain.
    All,
}

/// What the `-do` scripts ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DoPlan {
    pub run: DoRun,
    /// One warning per kind of command accepted without effect.
    pub warnings: Vec<String>,
}

/// Commands accepted as no-ops: waveform logging (xezim dumps through
/// `$dumpvars` with `--wave`/`--fst`) and coverage database files.
fn no_op_command(words: &[&str]) -> Option<(&'static str, &'static str)> {
    const WAVES: &str = "use --fst (or --wave for $dumpvars) for waveforms";
    const COVERAGE: &str = "xezim writes its coverage to xezim_cov.json (XEZIM_COV_DB) at the end";
    match words {
        ["log", ..] => Some(("log", WAVES)),
        ["add", "wave", ..] => Some(("add wave", WAVES)),
        ["coverage", "save", ..] => Some(("coverage save", COVERAGE)),
        ["coverage", "report", ..] => Some(("coverage report", COVERAGE)),
        _ => None,
    }
}

/// The accepted `-do` subset: `run -all`, `run <n><unit>` (also `run <n>
/// <unit>`), `quit`/`exit` (optionally `-f`/`-force`/`-sim`) and `do <file>`,
/// separated by `;` or newlines, with `#` comments; `log`, `add wave`,
/// `coverage save` and `coverage report` are accepted with a warning and do
/// nothing. Anything else is an error, never silently skipped.
pub(crate) fn plan_do_scripts(scripts: &[(String, String)]) -> Result<DoPlan, String> {
    let mut plan = DoPlan {
        run: DoRun::Load,
        warnings: Vec::new(),
    };
    let mut warned: Vec<&'static str> = Vec::new();
    let mut depth = 0;
    for (text, origin) in scripts {
        if walk_do_script(text, origin, &mut plan, &mut warned, &mut depth)? {
            break;
        }
    }
    Ok(plan)
}

/// Returns true once the script quit.
fn walk_do_script(
    text: &str,
    origin: &str,
    plan: &mut DoPlan,
    warned: &mut Vec<&'static str>,
    depth: &mut u32,
) -> Result<bool, String> {
    for line in text.lines() {
        for cmd in line.split(';') {
            let cmd = cmd.trim();
            if cmd.starts_with('#') {
                break;
            }
            let words: Vec<&str> = cmd.split_whitespace().collect();
            let Some((&head, args)) = words.split_first() else {
                continue;
            };
            if let Some((kind, why)) = no_op_command(&words) {
                if !warned.contains(&kind) {
                    warned.push(kind);
                    plan.warnings
                        .push(format!("{}: `{}` is ignored: {}", origin, cmd, why));
                }
                continue;
            }
            match head {
                "run" => {
                    let step = parse_run_args(args)
                        .map_err(|e| format!("{}: `{}`: {}", origin, cmd, e))?;
                    plan.run = match (plan.run, step) {
                        (DoRun::All, _) | (_, DoRun::All) => DoRun::All,
                        (DoRun::For(a), DoRun::For(b)) => DoRun::For(a.saturating_add(b)),
                        (DoRun::Load, s) | (s, DoRun::Load) => s,
                    };
                }
                "quit" | "exit" => {
                    if let Some(bad) = args
                        .iter()
                        .find(|a| !matches!(**a, "-f" | "-force" | "-sim"))
                    {
                        return Err(format!(
                            "{}: `{}`: unsupported {} option '{}'",
                            origin, cmd, head, bad
                        ));
                    }
                    return Ok(true);
                }
                "do" => {
                    let [file] = args else {
                        return Err(format!("{}: `{}`: expected `do <file>`", origin, cmd));
                    };
                    *depth += 1;
                    if *depth > 16 {
                        return Err(format!("{}: `do` nested too deeply", origin));
                    }
                    let nested = std::fs::read_to_string(file)
                        .map_err(|e| format!("{}: cannot read '{}': {}", origin, file, e))?;
                    let quit =
                        walk_do_script(&nested, &format!("do {}", file), plan, warned, depth)?;
                    *depth -= 1;
                    if quit {
                        return Ok(true);
                    }
                }
                _ => {
                    return Err(format!(
                        "{}: unsupported command `{}`; xezim runs the -do subset \
                         `run -all`, `run <n><unit>`, `quit [-f]`, `exit [-f]`, `do <file>` \
                         (and ignores `log`, `add wave`, `coverage save`, `coverage report`)",
                        origin, cmd
                    ));
                }
            }
        }
    }
    Ok(false)
}

/// `run` arguments: `-all`, or a time with a unit (`100ns`, `100 ns`,
/// `1.5us`). A bare number is refused: its unit would be the design's time
/// precision, which is easy to get wrong silently.
fn parse_run_args(args: &[&str]) -> Result<DoRun, String> {
    let joined = args.concat();
    match joined.as_str() {
        "-all" | "-a" => return Ok(DoRun::All),
        "" => {
            return Err("give `run -all` or `run <n><unit>`".to_string());
        }
        _ => {}
    }
    let split = joined
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or_else(|| {
            format!(
                "'{}' has no time unit; write e.g. `run {}ns`",
                joined, joined
            )
        })?;
    let (num, unit) = joined.split_at(split);
    let ns_per_unit = match unit {
        "fs" => 1e-6,
        "ps" => 1e-3,
        "ns" => 1.0,
        "us" => 1e3,
        "ms" => 1e6,
        "s" | "sec" => 1e9,
        "min" => 60e9,
        "hr" => 3600e9,
        _ => return Err(format!("unknown time unit '{}'", unit)),
    };
    let n: f64 = num
        .parse()
        .map_err(|_| format!("invalid run time '{}'", joined))?;
    let ns = n * ns_per_unit;
    if !(ns >= 0.0) || !ns.is_finite() {
        return Err(format!("invalid run time '{}'", joined));
    }
    if (ns - ns.round()).abs() > 1e-6 {
        return Err(format!(
            "'{}' is not a whole number of ns; xezim stops runs on whole ns",
            joined
        ));
    }
    Ok(DoRun::For(ns.round() as u64))
}
