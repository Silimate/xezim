//! Running xezim for a tool call: the arguments, the child process, and what
//! its log says.

use super::Live;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The default time a run may take, in seconds.
const TIMEOUT: u64 = 600;
/// The most diagnostics a result lists; the counts cover them all.
const DIAGNOSTICS: usize = 50;
/// The most of a log read for its diagnostics: its first and last halves.
const READ: u64 = 32 << 20;

/// One tool call: its request, and where its runs keep their files.
pub(crate) struct Job {
    pub key: String,
    pub live: Live,
    pub root: PathBuf,
}

/// A finished run.
pub(crate) struct Run {
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub log: PathBuf,
    pub text: String,
    pub tail: usize,
    /// The results file the run was told to write.
    pub db: PathBuf,
    /// cocotb's JUnit results, for a cocotb run.
    pub junit: Option<PathBuf>,
}

/// The server's scratch directory, removed when it exits.
pub(crate) fn root() -> PathBuf {
    std::env::temp_dir().join(format!("xezim-mcp-{}", std::process::id()))
}

/// Stop a child and anything it started (each child leads its own process
/// group).
pub(crate) fn kill(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

pub(crate) fn strings(args: &Value, key: &str) -> Result<Vec<String>, String> {
    match &args[key] {
        Value::Null => Ok(vec![]),
        Value::Array(a) => a
            .iter()
            .map(|v| match v {
                Value::String(s) => Ok(s.clone()),
                Value::Number(n) => Ok(n.to_string()),
                _ => Err(format!("`{key}` must be a list of strings")),
            })
            .collect(),
        _ => Err(format!("`{key}` must be a list of strings")),
    }
}

pub(crate) fn string(args: &Value, key: &str) -> Result<Option<String>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        Value::Number(n) => Ok(Some(n.to_string())),
        _ => Err(format!("`{key}` must be a string")),
    }
}

pub(crate) fn int(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        v => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("`{key}` must be a non-negative integer")),
    }
}

/// The directory a call runs in.
pub(crate) fn cwd(args: &Value) -> Result<PathBuf, String> {
    let here = std::env::current_dir().map_err(|e| format!("no working directory: {e}"))?;
    let dir = string(args, "cwd")?.map_or(here.clone(), |d| here.join(d));
    if !dir.is_dir() {
        return Err(format!("`cwd` {} is not a directory", dir.display()));
    }
    Ok(dir)
}

/// Run xezim with the call's arguments. `extra` goes before the caller's own
/// `args`, so those can override it.
pub(crate) fn execute(
    args: &Value,
    job: &Job,
    simulate: bool,
    extra: &[String],
) -> Result<Run, String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let cwd = cwd(args)?;
    let sources = strings(args, "sources")?;
    let more = strings(args, "args")?;
    if sources.is_empty() && more.is_empty() {
        return Err("give `sources`, or `args` such as [\"-f\", \"files.f\"]".into());
    }
    let top = string(args, "top")?;
    let seed = int(args, "seed")?;
    let limit = Duration::from_secs(int(args, "timeout")?.unwrap_or(TIMEOUT));
    let tail = int(args, "log_lines")?.unwrap_or(40) as usize;
    let dir = job
        .root
        .join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let db = dir.join("xezim_cov.json");

    let mut argv: Vec<String> = vec![
        if simulate {
            "--error-exit"
        } else {
            "--compile"
        }
        .into(),
    ];
    for d in strings(args, "include_dirs")? {
        argv.extend(["-I".into(), d]);
    }
    for d in strings(args, "defines")? {
        argv.extend(["-D".into(), d]);
    }
    for p in strings(args, "parameters")? {
        if !p.contains('=') {
            return Err(format!("parameter `{p}` must be NAME=VALUE"));
        }
        argv.push(format!("-g{p}"));
    }
    if let Some(t) = &top {
        argv.extend(["-s".into(), t.clone()]);
    }
    let mut env = vec![("XEZIM_COV_DB".to_string(), db.display().to_string())];
    let mut junit = None;
    if simulate {
        if let Some(t) = string(args, "max_time")? {
            argv.extend(["--max-time".into(), t]);
        }
        if let Some(s) = seed {
            argv.extend(["-sv_seed".into(), s.to_string()]);
        }
        if !args["cocotb"].is_null() {
            let (vpi, vars) = cocotb(&args["cocotb"], top.as_deref(), seed, &cwd, &dir)?;
            argv.extend(["--vpi-lib".into(), vpi]);
            env.extend(vars);
            junit = Some(dir.join("results.xml"));
        }
    }
    argv.extend(extra.iter().cloned());
    argv.extend(more);
    argv.extend(sources);
    if simulate {
        argv.extend(strings(args, "plusargs")?);
    }

    let exe = std::env::current_exe().map_err(|e| format!("cannot find xezim: {e}"))?;
    let log = dir.join("xezim.log");
    let file =
        std::fs::File::create(&log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
    let mut cmd = Command::new(exe);
    cmd.args(&argv)
        .current_dir(&cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(file.try_clone().map_err(|e| e.to_string())?)
        .stderr(file);
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            // Its own process group, so a kill reaches what it starts; and
            // killed with the server, so a crashed server leaves no orphans.
            libc::setpgid(0, 0);
            #[cfg(target_os = "linux")]
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot run xezim: {e}"))?;
    let pid = child.id();
    let set = |pid: Option<u32>| match job.live.lock().unwrap().get_mut(&job.key) {
        Some(slot) => {
            *slot = pid;
            true
        }
        None => false,
    };
    if !set(Some(pid)) {
        kill(pid);
        let _ = child.wait();
        return Err("cancelled".into());
    }
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(Some(s)),
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            // Out of time, or the child can't be waited on: stop it.
            done => {
                kill(pid);
                let _ = child.wait();
                break done.map(|_| None);
            }
        }
    };
    set(None);
    let status = status.map_err(|e| format!("cannot wait for xezim: {e}"))?;
    use std::os::unix::process::ExitStatusExt;
    Ok(Run {
        exit: status.and_then(|s| s.code()),
        signal: status.and_then(|s| s.signal()),
        timed_out: status.is_none(),
        text: read(&log),
        log,
        tail,
        db,
        junit,
    })
}

/// Point xezim at cocotb's VPI library and set the variables cocotb's own
/// runners set (cocotb 2).
fn cocotb(
    cfg: &Value,
    top: Option<&str>,
    seed: Option<u64>,
    cwd: &Path,
    dir: &Path,
) -> Result<(String, Vec<(String, String)>), String> {
    let module = string(cfg, "module")?.ok_or("`cocotb.module` is required")?;
    let top = top.ok_or("a cocotb run needs `top`")?;
    let python = string(cfg, "python")?.unwrap_or_else(|| "python3".into());
    // cocotb's plain-VPI build of its library, the one contrib/cocotb's
    // runner loads too (and overridden the same way).
    let flavour = std::env::var("XEZIM_COCOTB_VPI_FLAVOUR").unwrap_or_else(|_| "vcs".into());
    let query = |flags: &[&str]| -> Result<String, String> {
        let out = Command::new(&python)
            .args(["-m", "cocotb_tools.config"])
            .args(flags)
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("cannot run {python}: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{python} has no cocotb 2 (cocotb_tools.config {}): {}",
                flags.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let vpi = query(&["--lib-name-path", "vpi", &flavour])?;
    let libs = Path::new(&vpi)
        .parent()
        .unwrap_or(Path::new("."))
        .display()
        .to_string();
    let join = |first: String, var: &str| match std::env::var(var) {
        Ok(v) if !v.is_empty() => format!("{first}:{v}"),
        _ => first,
    };
    let mut env = vec![
        ("COCOTB_TEST_MODULES".into(), module),
        ("COCOTB_TOPLEVEL".into(), top.to_string()),
        ("TOPLEVEL_LANG".into(), "verilog".into()),
        ("PYGPI_PYTHON_BIN".into(), query(&["--python-bin"])?),
        ("LIBPYTHON_LOC".into(), query(&["--libpython"])?),
        (
            "COCOTB_RESULTS_FILE".into(),
            dir.join("results.xml").display().to_string(),
        ),
        ("LD_LIBRARY_PATH".into(), join(libs, "LD_LIBRARY_PATH")),
        (
            "PYTHONPATH".into(),
            join(cwd.display().to_string(), "PYTHONPATH"),
        ),
    ];
    if let Some(f) = string(cfg, "filter")? {
        env.push(("COCOTB_TEST_FILTER".into(), f));
    }
    if let Some(s) = seed {
        env.push(("COCOTB_RANDOM_SEED".into(), s.to_string()));
    }
    Ok((vpi, env))
}

/// A log's text: all of it, or its first and last halves of `READ` bytes.
fn read(path: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut buf = Vec::new();
    if size <= READ {
        let _ = f.read_to_end(&mut buf);
        return String::from_utf8_lossy(&buf).into_owned();
    }
    let _ = (&mut f).take(READ / 2).read_to_end(&mut buf);
    buf.extend_from_slice(b"\n[... log cut ...]\n");
    let _ = f.seek(SeekFrom::End(-((READ / 2) as i64)));
    let _ = f.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// The last `n` lines of a log, at most 16 KiB of them.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let out = lines[lines.len().saturating_sub(n)..].join("\n");
    let mut at = out.len().saturating_sub(16 << 10);
    while !out.is_char_boundary(at) {
        at += 1;
    }
    out[at..].to_string()
}

/// `file:line[:col]` at the start of `text`, when `file` looks like a source.
fn location(text: &str) -> Option<(String, u64, Option<u64>)> {
    let mut parts = text.splitn(4, ':');
    let file = parts.next()?;
    let line = parts.next()?.parse().ok()?;
    let col = parts.next().and_then(|c| c.parse().ok());
    let ext = Path::new(file).extension()?.to_str()?;
    let hdl = [
        "sv", "v", "svh", "vh", "sva", "svi", "svp", "vlib", "inc", "upf",
    ];
    hdl.contains(&ext).then(|| (file.to_string(), line, col))
}

/// The first `file:line[:col]` a message mentions.
fn mentioned(msg: &str) -> Option<(String, u64, Option<u64>)> {
    msg.split(|c: char| c.is_whitespace() || "()[],;'\"".contains(c))
        .find_map(location)
}

fn diagnostic(severity: &str, msg: &str, at: Option<(String, u64, Option<u64>)>) -> Value {
    let mut d = json!({ "severity": severity, "message": msg.trim() });
    if let Some((file, line, col)) = at {
        d["file"] = json!(file);
        d["line"] = json!(line);
        if let Some(c) = col {
            d["column"] = json!(c);
        }
    }
    d
}

/// The errors and warnings in a log: located parser and elaboration
/// messages (`file:line:col: error: ...`), xezim's `[xezim][warning]` and
/// `Error:` lines, and the run's `** Error:` / `** Fatal:` / `** Warning:`
/// messages with their time and scope.
pub(crate) fn diagnostics(text: &str) -> Vec<Value> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim_end();
        let found = ["error", "fatal", "warning"].iter().find_map(|sev| {
            let at = line.find(&format!(": {sev}: "))?;
            let loc = location(&line[..at])?;
            Some(diagnostic(sev, &line[at + sev.len() + 4..], Some(loc)))
        });
        if let Some(d) = found {
            out.push(d);
            continue;
        }
        if let Some(rest) = line.strip_prefix("** ") {
            let Some((sev, msg)) = rest.split_once(": ") else {
                continue;
            };
            let sev = match sev {
                "Error" => "error",
                "Fatal" => "fatal",
                "Warning" => "warning",
                _ => continue,
            };
            let mut d = diagnostic(sev, msg, mentioned(msg));
            // `   Time: 25  Scope: tb.a_gnt`, or `   Time: 25 ns  Scope: ...`
            if let Some(next) = lines
                .get(i + 1)
                .and_then(|l| l.trim().strip_prefix("Time: "))
            {
                let (time, scope) = next.split_once("Scope: ").unwrap_or((next, ""));
                d["time"] = json!(time.trim());
                if !scope.trim().is_empty() {
                    d["scope"] = json!(scope.trim());
                }
            }
            out.push(d);
            continue;
        }
        let tagged = [
            ("[xezim][error] ", "error"),
            ("[xezim][warning] ", "warning"),
            ("Simulation error: ", "error"),
            ("Error: ", "error"),
            ("Warning: ", "warning"),
        ];
        if let Some((msg, sev)) = tagged
            .iter()
            .find_map(|(p, s)| line.strip_prefix(p).map(|m| (m, *s)))
        {
            out.push(diagnostic(sev, msg, mentioned(msg)));
        }
    }
    out
}

/// The result fields every run has.
pub(crate) fn report(run: &Run) -> Map<String, Value> {
    let diags = diagnostics(&run.text);
    let count = |sev: &[&str]| {
        diags
            .iter()
            .filter(|d| sev.contains(&d["severity"].as_str().unwrap_or("")))
            .count()
    };
    let mut r = Map::new();
    r.insert("exit_code".into(), json!(run.exit));
    if let Some(s) = run.signal {
        r.insert("signal".into(), json!(s));
    }
    r.insert("timed_out".into(), json!(run.timed_out));
    r.insert("errors".into(), json!(count(&["error", "fatal"])));
    r.insert("warnings".into(), json!(count(&["warning"])));
    r.insert(
        "diagnostics".into(),
        json!(diags.into_iter().take(DIAGNOSTICS).collect::<Vec<_>>()),
    );
    r.insert("log".into(), json!(run.log.display().to_string()));
    r.insert("log_tail".into(), json!(tail(&run.text, run.tail)));
    r
}

pub(crate) fn compile(args: &Value, job: &Job) -> Result<Value, String> {
    let run = execute(args, job, false, &[])?;
    let mut r = report(&run);
    r.insert("ok".into(), json!(run.exit == Some(0)));
    Ok(Value::Object(r))
}

/// A simulation's result: the run's fields, how it ended, its assertion
/// failures and, for cocotb, its tests.
pub(crate) fn outcome(run: &Run) -> Map<String, Value> {
    let mut r = report(run);
    // `Simulation finished at time N` closes every run that got that far.
    let end = run
        .text
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix("Simulation finished at time "));
    r.insert(
        "sim_time".into(),
        json!(end.and_then(|e| e.split(' ').next()?.parse::<u64>().ok())),
    );
    r.insert(
        "finished".into(),
        json!(end.is_some_and(|e| e.contains("$finish"))),
    );
    let failed = match super::cov::load(&run.db) {
        Ok(db) => {
            let a = super::cov::summary(&db);
            let n = a["failed"].as_u64().unwrap_or(0);
            r.insert("assertions".into(), a);
            n
        }
        Err(_) => {
            r.insert("assertions".into(), Value::Null);
            0
        }
    };
    let mut ok = run.exit == Some(0) && !run.timed_out && failed == 0;
    if let Some(path) = &run.junit {
        let tests = junit(&std::fs::read_to_string(path).unwrap_or_default());
        // A run whose tests were all filtered out or skipped is no pass.
        ok &= tests["passed"].as_u64() > Some(0) && tests["failed"] == 0;
        r.insert("tests".into(), tests);
    }
    r.insert("passed".into(), json!(ok));
    r
}

pub(crate) fn simulate(args: &Value, job: &Job) -> Result<Value, String> {
    Ok(Value::Object(outcome(&execute(args, job, true, &[])?)))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", "\n")
        .replace("&amp;", "&")
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let len = tag[at..].find('"')?;
    Some(unescape(&tag[at..at + len]))
}

/// The test cases of cocotb's JUnit results file.
fn junit(xml: &str) -> Value {
    let mut cases = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<testcase") {
        rest = &rest[at..];
        let close = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..close];
        let body = if tag.ends_with('/') {
            ""
        } else {
            let end = rest.find("</testcase>").unwrap_or(rest.len());
            &rest[close..end]
        };
        let status = if body.contains("<failure") || body.contains("<error") {
            "failed"
        } else if body.contains("<skipped") {
            "skipped"
        } else {
            "passed"
        };
        let mut case = json!({
            "name": attr(tag, "name").unwrap_or_default(),
            "module": attr(tag, "classname").unwrap_or_default(),
            "status": status,
        });
        let message = ["<failure", "<error"].iter().find_map(|t| {
            let i = body.find(t)?;
            let end = body[i..].find('>').map_or(body.len(), |j| i + j);
            attr(&body[i..end], "message")
        });
        if let Some(m) = message {
            case["message"] = json!(m);
        }
        cases.push(case);
        rest = &rest[close.min(rest.len())..];
    }
    let n = |s: &str| cases.iter().filter(|c| c["status"] == s).count();
    json!({
        "total": cases.len(),
        "passed": n("passed"),
        "failed": n("failed"),
        "skipped": n("skipped"),
        "cases": cases,
    })
}
