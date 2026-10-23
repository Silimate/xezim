//! The `coverage` and `assertions` tools: a results file (`xezim_cov.json`,
//! see docs/coverage-guide.md) normalized per instance, with its holes.

use super::run::{self, Job};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// xezim's code coverage metrics, in the order holes are listed.
const METRICS: [&str; 6] = [
    "statement",
    "branch",
    "condition",
    "expression",
    "fsm",
    "toggle",
];

/// The counting options of each `counting` profile. `reference` is the
/// reference simulator's documented defaults (docs/coverage-guide.md,
/// "Counting rules"); `xezim` spells out xezim's own defaults, so that the
/// server's environment can't change them.
fn counting(profile: &str) -> Result<&'static [&'static str], String> {
    match profile {
        "xezim" => Ok(&[
            "--toggle-enums",
            "--toggle-generate",
            "--toggle-tied",
            "--code-coverage-expressions=all",
            "--code-coverage-granularity=statement",
            "--fsm-reset-transitions",
        ]),
        "reference" => Ok(&[
            "--no-toggle-enums",
            "--no-toggle-generate",
            "--no-toggle-tied",
            "--code-coverage-expressions=logical",
            "--code-coverage-granularity=block",
            "--no-fsm-reset-transitions",
        ]),
        _ => Err(format!(
            "`counting` must be xezim or reference, not {profile}"
        )),
    }
}

/// A results file, or the `xezim_cov.json` in a directory.
pub(crate) fn load(path: &Path) -> Result<Value, String> {
    let file = if path.is_dir() {
        path.join("xezim_cov.json")
    } else {
        path.to_path_buf()
    };
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let db: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    // Every results file has the run's end time and its assertion list.
    if db.get("sim_time").is_none() || db.get("assertions").is_none() {
        return Err(format!("{} is not an xezim results file", file.display()));
    }
    Ok(db)
}

/// The results to report on: `db` when given, else a fresh run's, with that
/// run's outcome under `run`.
fn source(
    args: &Value,
    job: &Job,
    extra: &[String],
) -> Result<(Option<Value>, Map<String, Value>), String> {
    let mut r = Map::new();
    if let Some(p) = run::string(args, "db")? {
        let path = run::cwd(args)?.join(p);
        let db = load(&path)?;
        r.insert("db".into(), json!(path.display().to_string()));
        return Ok((Some(db), r));
    }
    let done = run::execute(args, job, true, extra)?;
    let db = load(&done.db).ok();
    r.insert(
        "db".into(),
        json!(db.as_ref().map(|_| done.db.display().to_string())),
    );
    r.insert("run".into(), Value::Object(run::outcome(&done)));
    Ok((db, r))
}

/// Whether `scope` is `inst` or below it; `inst` is a dotted path or a leaf
/// name.
fn inside(scope: &str, inst: Option<&str>) -> bool {
    let Some(inst) = inst else { return true };
    let parts: Vec<&str> = scope.split('.').collect();
    (1..=parts.len()).any(|k| {
        let head = parts[..k].join(".");
        head == inst || head.ends_with(&format!(".{inst}"))
    })
}

fn n(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}

fn metric(covered: u64, total: u64) -> Value {
    let percent = (total > 0).then(|| (covered as f64 * 10000.0 / total as f64).round() / 100.0);
    json!({ "covered": covered, "total": total, "percent": percent })
}

fn status(a: &Value) -> &'static str {
    if a["kind"] == "cover" {
        return if n(a, "pass") > 0 {
            "covered"
        } else {
            "uncovered"
        };
    }
    if n(a, "fail") > 0 {
        "failed"
    } else if n(a, "pass") > 0 {
        "passed"
    } else if n(a, "vacuous") > 0 {
        "vacuous"
    } else {
        "unattempted"
    }
}

/// Covered as the reference simulator scores an assertion: a nonvacuous
/// pass (a match, for a cover) and no failure.
fn covered(a: &Value) -> bool {
    n(a, "pass") > 0 && n(a, "fail") == 0
}

fn entries(db: &Value) -> Vec<Value> {
    db["assertion_instances"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// Counts by status, and the failures.
pub(crate) fn summary(db: &Value) -> Value {
    tally(&entries(db))
}

fn tally(list: &[Value]) -> Value {
    let count = |s: &str| list.iter().filter(|a| status(a) == s).count();
    let failures: Vec<Value> = list
        .iter()
        .filter(|a| n(a, "fail") > 0)
        .take(20)
        .map(|a| json!({ "instance": a["instance"], "name": a["name"], "kind": a["kind"], "file": a["file"], "line": a["line"], "fail": a["fail"] }))
        .collect();
    json!({
        "total": list.len(),
        "passed": count("passed"),
        "failed": count("failed"),
        "vacuous": count("vacuous"),
        "unattempted": count("unattempted"),
        "covered": count("covered"),
        "uncovered": count("uncovered"),
        "failures": failures,
    })
}

pub(crate) fn assertions(args: &Value, job: &Job) -> Result<Value, String> {
    let (db, mut r) = source(args, job, &[])?;
    let Some(db) = db else {
        return Ok(Value::Object(r));
    };
    // The run's summary would repeat this tool's own.
    if let Some(Value::Object(run)) = r.get_mut("run") {
        run.remove("assertions");
    }
    if db.get("assertion_instances").is_none() {
        return Err("the results file has no per-instance assertion results".into());
    }
    let inst = run::string(args, "instance")?;
    let list: Vec<Value> = entries(&db)
        .into_iter()
        .filter(|a| inside(a["instance"].as_str().unwrap_or(""), inst.as_deref()))
        .map(|a| {
            json!({
                "instance": a["instance"], "name": a["name"], "kind": a["kind"],
                "concurrent": a["concurrent"], "file": a["file"], "line": a["line"],
                "attempts": a["attempts"], "pass": a["pass"], "vacuous": a["vacuous"],
                "fail": a["fail"], "disabled": a["disabled"], "status": status(&a),
            })
        })
        .collect();
    r.insert("summary".into(), tally(&list));
    r.insert("assertions".into(), json!(list));
    Ok(Value::Object(r))
}

/// Entries the totals count: xezim marks the ones a constant rules out.
fn kept(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|x| x.get("excluded").is_none())
}

/// The uncovered bins of one instance's code coverage.
fn holes(s: &Value) -> Vec<Value> {
    let at = &s["scope"];
    let mut out = Vec::new();
    for x in kept(&s["statements"]).filter(|x| n(x, "count") == 0) {
        out.push(json!({ "metric": "statement", "file": x["file"], "line": x["line"], "kind": x["kind"] }));
    }
    for b in kept(&s["branches"]) {
        for a in kept(&b["arms"]).filter(|a| n(a, "count") == 0) {
            out.push(json!({ "metric": "branch", "file": b["file"], "line": a["line"], "kind": b["kind"], "arm": a["arm"] }));
        }
    }
    for (key, metric, rows) in [
        ("conditions", "condition", ["false", "true"]),
        ("expressions", "expression", ["0", "1"]),
    ] {
        for e in kept(&s[key]) {
            for t in kept(&e["terms"]) {
                let hits = t["hits"].as_array().cloned().unwrap_or_default();
                let missing: Vec<&str> = rows
                    .iter()
                    .zip(&hits)
                    .filter(|(_, h)| h.as_u64() == Some(0))
                    .map(|(r, _)| *r)
                    .collect();
                if !missing.is_empty() {
                    out.push(json!({ "metric": metric, "file": e["file"], "line": e["line"], "text": e["text"], "term": t["term"], "missing": missing }));
                }
            }
        }
    }
    for f in kept(&s["fsms"]) {
        for x in kept(&f["states"]).filter(|x| n(x, "count") == 0) {
            out.push(json!({ "metric": "fsm", "variable": f["variable"], "state": x["name"], "line": x["line"] }));
        }
        for x in kept(&f["transitions"]).filter(|x| n(x, "count") == 0) {
            out.push(json!({ "metric": "fsm", "variable": f["variable"], "transition": format!("{} -> {}", x["from"].as_str().unwrap_or(""), x["to"].as_str().unwrap_or("")), "line": x["line"] }));
        }
    }
    // Bits only constants drive are listed in `tied`; they are out of the
    // totals when the run left them out.
    let untied = n(&s["toggle"], "excluded") == 0;
    for t in kept(&s["toggles"]) {
        let tied: Vec<u64> = t["tied"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        let (rise, fall) = (t["rise"].as_array(), t["fall"].as_array());
        let (Some(rise), Some(fall)) = (rise, fall) else {
            continue;
        };
        for (bit, (r, f)) in rise.iter().zip(fall).enumerate() {
            if !untied && tied.contains(&(bit as u64)) {
                continue;
            }
            let missing: Vec<&str> = [("rise", r), ("fall", f)]
                .iter()
                .filter(|(_, c)| c.as_u64() == Some(0))
                .map(|(d, _)| *d)
                .collect();
            if missing.is_empty() {
                continue;
            }
            let name = t["signal"].as_str().unwrap_or("");
            let signal = if n(t, "width") > 1 {
                format!("{name}[{bit}]")
            } else {
                name.to_string()
            };
            out.push(json!({ "metric": "toggle", "signal": signal, "missing": missing }));
        }
    }
    for h in &mut out {
        h["instance"] = at.clone();
    }
    out
}

pub(crate) fn coverage(args: &Value, job: &Job) -> Result<Value, String> {
    let profile = run::string(args, "counting")?;
    let mut extra: Vec<String> = vec!["--code-coverage".into()];
    extra.extend(
        counting(profile.as_deref().unwrap_or("xezim"))?
            .iter()
            .map(|s| s.to_string()),
    );
    for s in run::strings(args, "coverage_scope")? {
        extra.push(format!("--code-coverage-scope={s}"));
    }
    let (db, mut r) = source(args, job, &extra)?;
    if r.contains_key("run") {
        r.insert(
            "counting".into(),
            json!(profile.as_deref().unwrap_or("xezim")),
        );
    }
    let Some(db) = db else {
        return Ok(Value::Object(r));
    };
    let code = &db["code_coverage"];
    let inst = run::string(args, "instance")?;
    let max = run::int(args, "max_holes")?.unwrap_or(100) as usize;

    // Each instance's own counts, and its holes.
    let mut own: BTreeMap<String, (Value, BTreeMap<&str, (u64, u64)>, Vec<Value>)> =
        BTreeMap::new();
    for s in code["scopes"].as_array().into_iter().flatten() {
        let scope = s["scope"].as_str().unwrap_or("");
        if s["kind"] != "instance" || !inside(scope, inst.as_deref()) {
            continue;
        }
        let e = own
            .entry(scope.to_string())
            .or_insert_with(|| (s["design_unit"].clone(), BTreeMap::new(), vec![]));
        for m in METRICS {
            if n(&s[m], "total") > 0 {
                e.1.insert(m, (n(&s[m], "covered"), n(&s[m], "total")));
            }
        }
        e.2.extend(holes(s));
    }
    let mut all = (0, 0);
    for a in entries(&db) {
        let c = covered(&a) as u64;
        all = (all.0 + c, all.1 + 1);
        let scope = a["instance"].as_str().unwrap_or("").to_string();
        if !inside(&scope, inst.as_deref()) {
            continue;
        }
        let e = own
            .entry(scope)
            .or_insert_with(|| (Value::Null, BTreeMap::new(), vec![]));
        let m = e.1.entry("assertion").or_default();
        *m = (m.0 + c, m.1 + 1);
        if c == 0 {
            e.2.push(json!({ "metric": "assertion", "instance": a["instance"], "name": a["name"], "kind": a["kind"], "file": a["file"], "line": a["line"], "status": status(&a) }));
        }
    }

    // Every instance and the scopes above it, each with its subtree's totals:
    // an instance's counts go to it and to each scope above it.
    let mut subtrees: BTreeMap<String, BTreeMap<&str, (u64, u64)>> = BTreeMap::new();
    for (p, (_, m, _)) in &own {
        let parts: Vec<&str> = p.split('.').collect();
        for k in 1..=parts.len() {
            let path = parts[..k].join(".");
            if !inside(&path, inst.as_deref()) {
                continue;
            }
            let sum = subtrees.entry(path).or_default();
            for (k, (c, t)) in m {
                let e = sum.entry(*k).or_default();
                *e = (e.0 + c, e.1 + t);
            }
        }
    }
    if let (Some(i), true) = (&inst, subtrees.is_empty()) {
        return Err(format!("no instance {i} in the results"));
    }
    let mut paths: Vec<&String> = subtrees.keys().collect();
    paths.sort_by(|a, b| a.split('.').cmp(b.split('.')));
    let mut list = Vec::new();
    let mut found = Vec::new();
    for path in paths {
        let sum = &subtrees[path];
        // The reference simulator's overall average: blocks, expression terms
        // (conditions and expressions pooled), toggles, FSMs and assertions.
        let pool = |ks: &[&str]| {
            ks.iter()
                .filter_map(|k| sum.get(k))
                .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
        };
        let parts = [
            pool(&["statement"]),
            pool(&["condition", "expression"]),
            pool(&["toggle"]),
            pool(&["fsm"]),
            pool(&["assertion"]),
        ];
        let scored: Vec<f64> = parts
            .iter()
            .filter(|p| p.1 > 0)
            .map(|p| p.0 as f64 * 100.0 / p.1 as f64)
            .collect();
        let average = (!scored.is_empty())
            .then(|| (scored.iter().sum::<f64>() / scored.len() as f64 * 100.0).round() / 100.0);
        let (unit, mine, gaps) = own
            .get(path)
            .map_or((Value::Null, BTreeMap::new(), vec![]), |o| {
                (o.0.clone(), o.1.clone(), o.2.clone())
            });
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for h in &gaps {
            *counts
                .entry(h["metric"].as_str().unwrap_or("").to_string())
                .or_default() += 1;
        }
        let metrics = |m: &BTreeMap<&str, (u64, u64)>| -> Map<String, Value> {
            m.iter()
                .map(|(k, (c, t))| (k.to_string(), metric(*c, *t)))
                .collect()
        };
        list.push(json!({
            "instance": path,
            "design_unit": unit,
            "metrics": metrics(&mine),
            "subtree": metrics(sum),
            "average": average,
            "holes": counts,
        }));
        found.extend(gaps);
    }
    let order = |h: &Value| match h["metric"].as_str() {
        Some("assertion") => 0,
        Some(m) => {
            1 + METRICS
                .iter()
                .position(|x| *x == m)
                .unwrap_or(METRICS.len())
        }
        None => 99,
    };
    found.sort_by_key(|h| order(h));
    let mut total: Map<String, Value> = METRICS
        .iter()
        .filter(|m| n(&code[**m], "total") > 0)
        .map(|m| {
            (
                m.to_string(),
                metric(n(&code[*m], "covered"), n(&code[*m], "total")),
            )
        })
        .collect();
    if all.1 > 0 {
        total.insert("assertion".into(), metric(all.0, all.1));
    }
    r.insert("total".into(), Value::Object(total));
    r.insert("instances".into(), json!(list));
    r.insert("holes_total".into(), json!(found.len()));
    found.truncate(max);
    r.insert("holes".into(), json!(found));
    Ok(Value::Object(r))
}
