//! MCP (Model Context Protocol) server: `wireant --mcp` reads JSON-RPC 2.0 messages, one per
//! line, on stdin and writes the replies on stdout, with no window.  An MCP client (Claude
//! Code, Claude Desktop and others) starts the program and calls the tools below to build,
//! load, calculate and optimise an antenna.  The server keeps one current antenna between
//! calls.  Nothing else may be written to stdout; diagnostics go to stderr.

use crate::model::*;
use crate::mom::{self, CalcResult, Gain};
use crate::optimize::{self, Goals, OptVar, VarKind};
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

struct Server {
    ant: Antenna,
    path: Option<PathBuf>,
}

pub fn run() -> std::io::Result<()> {
    let mut ant = Antenna::example(0);
    // the program's default engine (File → Preferences), then WIREANT_ENGINE
    let pick = std::env::var("WIREANT_ENGINE").ok().and_then(|v| crate::settings::engine_from_key(&v)).or(crate::settings::get().default_engine);
    if let Some(e) = pick.filter(|e| crate::nec::engine_available(*e)) {
        ant.engine = e;
    }
    let mut srv = Server { ant, path: None };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let r: Vec<Value> = batch.iter().filter_map(|m| srv.handle(m)).collect();
                if r.is_empty() { None } else { Some(Value::Array(r)) }
            }
            Ok(msg) => srv.handle(&msg),
            Err(e) => Some(error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        if let Some(r) = reply {
            serde_json::to_writer(&mut out, &r)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

fn error(id: Value, code: i64, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
}

impl Server {
    /// One JSON-RPC message; None for notifications and replies.
    fn handle(&mut self, msg: &Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str);
        let id = msg.get("id").cloned();
        let Some(method) = method else {
            // a reply to a request we never send, or junk
            return id.map(|id| error(id, -32600, "Invalid request"));
        };
        let id = id?; // notifications (initialized, cancelled, ...) need no answer
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = if PROTOCOL_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSIONS[0] };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "wireant", "title": "wireant", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(name, &args) {
                    Ok(v) => Ok(json!({
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                        "structuredContent": v,
                        "isError": false,
                    })),
                    Err(CallError::Unknown) => Err((-32602, format!("Unknown tool: {name}"))),
                    Err(CallError::Failed(e)) => Ok(json!({
                        "content": [{ "type": "text", "text": e }],
                        "isError": true,
                    })),
                }
            }
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, m)) => error(id, code, &m),
        })
    }

    fn call(&mut self, name: &str, a: &Value) -> Result<Value, CallError> {
        Ok(match name {
            "list_examples" => json!({
                "examples": Antenna::example_names().iter().enumerate()
                    .map(|(i, n)| json!({ "index": i, "name": n })).collect::<Vec<_>>(),
                "engines": engines(),
            }),
            "new_antenna" => {
                self.ant = match a.get("example").and_then(Value::as_u64) {
                    Some(i) if (i as usize) < Antenna::example_names().len() => keep_engine(Antenna::example(i as usize), &self.ant),
                    Some(i) => return fail(format!("There is no example {i}; use list_examples.")),
                    None => keep_engine(Antenna::blank(), &self.ant),
                };
                self.path = None;
                self.summary()
            }
            "open_file" => {
                let p = PathBuf::from(str_arg(a, "path")?);
                let (ant, warnings) = open(&p).map_err(CallError::Failed)?;
                self.ant = keep_engine(ant, &self.ant);
                self.path = Some(p);
                let mut s = self.summary();
                s["warnings"] = json!(warnings);
                s
            }
            "save_file" => {
                let p = match a.get("path").and_then(Value::as_str) {
                    Some(p) => PathBuf::from(p),
                    None => self.path.clone().ok_or_else(|| CallError::Failed("No path given and the antenna wasn't opened from a file.".into()))?,
                };
                let p = save(&self.ant, &p).map_err(CallError::Failed)?;
                let ext = |e: &str| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(e));
                let left_out = crate::app::not_saved(&self.ant, ext("ez"), ext("nec"));
                self.path = Some(p.clone());
                if left_out.is_empty() {
                    json!({ "saved": p })
                } else {
                    json!({ "saved": p, "left_out": left_out, "note": "This file type can't hold these; save as .nec to keep everything." })
                }
            }
            "get_antenna" => json!({ "antenna": self.ant, "path": self.path, "summary": self.summary() }),
            "set_antenna" => {
                let v = a.get("antenna").cloned().ok_or_else(|| CallError::Failed("Missing 'antenna'.".into()))?;
                let ant: Antenna = serde_json::from_value(v).map_err(|e| CallError::Failed(format!("Not a valid antenna: {e}")))?;
                self.ant = checked(ant)?;
                self.summary()
            }
            "update_antenna" => {
                let patch = a.get("patch").ok_or_else(|| CallError::Failed("Missing 'patch'.".into()))?;
                let mut v = serde_json::to_value(&self.ant).unwrap();
                merge_patch(&mut v, patch);
                let ant: Antenna = serde_json::from_value(v).map_err(|e| CallError::Failed(format!("The patch doesn't give a valid antenna: {e}")))?;
                self.ant = checked(ant)?;
                self.summary()
            }
            "set_wire" => {
                let i = a.get("index").and_then(Value::as_u64).ok_or_else(|| CallError::Failed("Missing 'index'.".into()))? as usize;
                let patch = a.get("wire").ok_or_else(|| CallError::Failed("Missing 'wire'.".into()))?;
                if i > self.ant.wires.len() {
                    return fail(format!("Wire index {i} is past the end; there are {} wires (use {} to add one).", self.ant.wires.len(), self.ant.wires.len()));
                }
                let mut v = serde_json::to_value(self.ant.wires.get(i).cloned().unwrap_or_default()).unwrap();
                merge_patch(&mut v, patch);
                let w: Wire = serde_json::from_value(v).map_err(|e| CallError::Failed(format!("Not a valid wire: {e}")))?;
                let mut ant = self.ant.clone();
                if i == ant.wires.len() { ant.wires.push(w) } else { ant.wires[i] = w }
                self.ant = checked(ant)?;
                self.summary()
            }
            "delete_wire" => {
                let i = a.get("index").and_then(Value::as_u64).ok_or_else(|| CallError::Failed("Missing 'index'.".into()))? as usize;
                if i >= self.ant.wires.len() {
                    return fail(format!("There is no wire {i}."));
                }
                delete_wire(&mut self.ant, i);
                self.summary()
            }
            "calculate" => {
                let ant = self.with_options(a)?;
                let mut full = mom::calculate(&ant).map_err(CallError::Failed)?;
                let mut v = result_json(&full.result);
                v["power_budget"] = power_json(&full.power);
                v["warnings"] = json!(full.warnings);
                if a.get("patterns").and_then(Value::as_bool).unwrap_or(false) {
                    let step = a.get("pattern_step_deg").and_then(Value::as_f64).unwrap_or(5.0).clamp(1.0, 90.0) as usize;
                    let p = &mut full.patterns;
                    let cut = |c: &Vec<(f64, Gain)>| c.iter().step_by(step).map(|(ang, g)| json!([ang, db3(g.total), db3(g.hor), db3(g.ver)])).collect::<Vec<_>>();
                    v["patterns"] = json!({
                        "columns": ["angle_deg", "total_dbi", "horizontal_dbi", "vertical_dbi"],
                        "azimuth_cut": { "elevation_deg": p.el_cut_deg, "points": cut(&p.az_cut) },
                        "elevation_cut": {
                            "azimuth_deg": p.az_cut_deg,
                            "note": "angle from the front horizon, over the zenith (90) to the back horizon (180)",
                            "points": cut(&p.el_cut),
                        },
                    });
                }
                v
            }
            "sweep" => {
                let ant = self.with_options(a)?;
                let f0 = num_arg(a, "f_start_mhz")?;
                let f1 = num_arg(a, "f_stop_mhz")?;
                if !(f0 > 0.0 && f1 > 0.0) {
                    return fail("Frequencies must be above 0 MHz.".into());
                }
                let steps = a.get("steps").and_then(Value::as_u64).unwrap_or(21).clamp(2, 1001) as usize;
                let gain = a.get("with_gain").and_then(Value::as_bool).unwrap_or(true);
                let r = mom::sweep(&ant, f0, f1, steps, gain).map_err(CallError::Failed)?;
                let pts: Vec<Value> = r.points.iter().map(|p| {
                    let mut o = json!({ "freq_mhz": p.freq_mhz, "r_ohm": p.z.re, "x_ohm": p.z.im, "swr": fin(p.swr) });
                    if gain {
                        o["gain_dbi"] = json!(p.ga_dbi);
                        o["fb_db"] = json!(p.fb_db);
                    }
                    o
                }).collect();
                let best = r.points.iter().filter(|p| p.swr.is_finite()).min_by(|x, y| x.swr.total_cmp(&y.swr));
                json!({
                    "engine": engine_name(ant.engine),
                    "z0_ohm": ant.z0,
                    "points": pts,
                    "lowest_swr": best.map(|p| json!({ "freq_mhz": p.freq_mhz, "swr": p.swr })),
                    "time_ms": r.t_ms,
                })
            }
            "optimize" => {
                let vars: Vec<OptVar> = a.get("variables").and_then(Value::as_array).map(|v| v.iter().map(|x| {
                    let wire = x.get("wire").and_then(Value::as_u64).ok_or("Each variable needs 'wire'.")? as usize;
                    let kind = x.get("kind").and_then(Value::as_str).ok_or("Each variable needs 'kind'.")?;
                    let kind = var_kind(kind).ok_or("Unknown variable kind.")?;
                    Ok::<_, &str>(OptVar { wire, kind })
                }).collect::<Result<Vec<_>, _>>()).transpose().map_err(|e| CallError::Failed(e.into()))?.unwrap_or_default();
                if vars.is_empty() {
                    return fail("Give at least one variable.".into());
                }
                if let Some(v) = vars.iter().find(|v| v.wire >= self.ant.wires.len()) {
                    return fail(format!("There is no wire {}.", v.wire));
                }
                let g = a.get("goals").cloned().unwrap_or(json!({}));
                let d = Goals::default();
                let w = |k: &str, def: f64| g.get(k).and_then(Value::as_f64).unwrap_or(def);
                let goals = Goals {
                    w_swr: w("swr", d.w_swr),
                    w_jx: w("reactance", d.w_jx),
                    w_ga: w("gain", d.w_ga),
                    w_fb: w("front_to_back", d.w_fb),
                    max_iter: a.get("max_iterations").and_then(Value::as_u64).map(|n| n.clamp(1, 1000) as usize).unwrap_or(d.max_iter),
                };
                let base = self.with_options(a)?;
                let (tx, rx) = std::sync::mpsc::channel();
                optimize::run(base.clone(), vars, goals, Arc::new(AtomicBool::new(false)), tx, || {});
                let mut first = None;
                let mut last = None;
                let mut done = None;
                for m in rx.try_iter() {
                    match m {
                        optimize::Msg::Progress(s) => {
                            if first.is_none() { first = Some(s.clone()) }
                            last = Some(s);
                        }
                        optimize::Msg::Done { ant, steps, evals, ms, failed, error } => done = Some((ant, steps, evals, ms, failed, error)),
                        optimize::Msg::Failed(e) => return fail(e),
                    }
                }
                let Some((mut ant, steps, evals, ms, failed, error)) = done else { return fail("The optimizer stopped without a result.".into()) };
                let step_json = |s: &Option<optimize::Step>| s.as_ref().map(|s| json!({ "r_ohm": s.r, "x_ohm": s.x, "swr": fin(s.swr), "gain_dbi": s.ga, "fb_db": s.fb, "cost": s.cost }));
                let apply = a.get("apply").and_then(Value::as_bool).unwrap_or(true);
                ant.engine = self.ant.engine;
                ant.freq_mhz = self.ant.freq_mhz;
                let changed = ant.wires != self.ant.wires;
                let wires: Vec<Value> = ant.wires.iter().map(|w| json!({ "p1": w.p1, "p2": w.p2, "length_m": w.length() })).collect();
                if apply {
                    self.ant = ant;
                }
                json!({
                    "applied": apply,
                    "changed": changed,
                    "failed_trials": failed,
                    "first_error": error,
                    "iterations": steps,
                    "evaluations": evals,
                    "time_ms": ms,
                    "start": step_json(&first),
                    "end": step_json(&last),
                    "wires": wires,
                })
            }
            "report" => {
                let ant = self.with_options(a)?;
                let p = match a.get("path").and_then(Value::as_str) {
                    Some(p) => PathBuf::from(p),
                    None => match &self.path {
                        Some(src) => {
                            let (dir, name) = crate::app::report_target(Some(src), None);
                            dir.map(|d| d.join(&name)).unwrap_or_else(|| PathBuf::from(name))
                        }
                        None => return fail("No path given and the antenna hasn't been opened from or saved to a file.".into()),
                    },
                };
                let p = if p.extension().is_none() { p.with_extension("pdf") } else { p };

                let calc = mom::calculate(&ant).map_err(CallError::Failed)?;
                let mut st = crate::tools::ToolsState::default();
                st.for_antenna(&ant);
                // the match calculator starts from this calculation, as Tools → Match calculator does
                st.m_f = calc.result.freq_mhz;
                st.m_r = calc.result.z.re;
                st.m_x = calc.result.z.im;
                if a.get("resonances").and_then(Value::as_bool).unwrap_or(true) {
                    if let Some(f0) = a.get("res_f_start_mhz").and_then(Value::as_f64) {
                        st.res_f0 = f0;
                    }
                    if let Some(f1) = a.get("res_f_stop_mhz").and_then(Value::as_f64) {
                        st.res_f1 = f1;
                    }
                    if !(st.res_f0 > 0.0 && st.res_f1 > st.res_f0) {
                        return fail("The resonance range must be above 0 MHz with the stop above the start.".into());
                    }
                    if let Some(n) = a.get("res_points").and_then(Value::as_u64) {
                        st.res_steps = n.clamp(5, 1001) as usize;
                    }
                    if let Some(l) = a.get("swr_limit").and_then(Value::as_f64) {
                        st.res_limit = l.max(1.01);
                    }
                    let rep = crate::tools::find(&ant, st.res_f0, st.res_f1, st.res_steps, st.res_limit).map_err(CallError::Failed)?;
                    // mark the resonance nearest the antenna's frequency
                    st.sel = rep
                        .resonances
                        .iter()
                        .enumerate()
                        .min_by(|x, y| (x.1.f_mhz - ant.freq_mhz).abs().total_cmp(&(y.1.f_mhz - ant.freq_mhz).abs()))
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    st.report = Some(rep);
                }

                let history = [calc.result.clone()];
                let bytes = crate::report::build(&ant, self.path.as_deref(), Some(&calc), &history, &st, &crate::plots::ViewState::default());
                std::fs::write(&p, bytes).map_err(|e| CallError::Failed(format!("Couldn't save {}: {e}", p.display())))?;

                let mut v = json!({ "saved": p, "calculation": result_json(&calc.result) });
                v["calculation"]["power_budget"] = power_json(&calc.power);
                v["calculation"]["warnings"] = json!(calc.warnings);
                if let Some(rep) = &st.report {
                    let bw = |b: Option<(f64, f64)>| b.map(|(lo, hi)| json!({ "from_mhz": lo, "to_mhz": hi, "width_khz": (hi - lo) * 1000.0 }));
                    v["resonances"] = json!({
                        "f_start_mhz": st.res_f0,
                        "f_stop_mhz": st.res_f1,
                        "points": st.res_steps,
                        "swr_limit": rep.limit,
                        "found": rep.resonances.iter().map(|r| json!({
                            "freq_mhz": r.f_mhz,
                            "type": if r.series { "series" } else { "parallel" },
                            "r_ohm": r.z.re,
                            "x_ohm": r.z.im,
                            "swr": fin(r.swr),
                            "bandwidth": bw(r.bw_ref),
                            "bandwidth_matched": bw(r.bw_matched),
                            "q": fin(r.q),
                        })).collect::<Vec<_>>(),
                        "lowest_swr": { "freq_mhz": rep.min_swr.0, "swr": fin(rep.min_swr.1) },
                    });
                }
                v
            }
            _ => return Err(CallError::Unknown),
        })
    }

    /// The current antenna with this call's optional engine and frequency.
    fn with_options(&self, a: &Value) -> Result<Antenna, CallError> {
        let mut ant = self.ant.clone();
        if let Some(e) = a.get("engine").and_then(Value::as_str) {
            ant.engine = parse_engine(e)?;
        }
        if let Some(f) = a.get("freq_mhz").and_then(Value::as_f64) {
            if !(f > 0.0) {
                return fail("The frequency must be above 0 MHz.".into());
            }
            ant.freq_mhz = f;
        }
        if ant.wires.is_empty() {
            return fail("The antenna has no wires.".into());
        }
        if ant.sources.is_empty() {
            return fail("The antenna has no source.".into());
        }
        Ok(ant)
    }

    fn summary(&self) -> Value {
        let a = &self.ant;
        json!({
            "name": a.name,
            "freq_mhz": a.freq_mhz,
            "wavelength_m": a.lambda(),
            "wires": a.wires.len(),
            "sources": a.sources.iter().map(|s| pos_label(s.wire, s.pos)).collect::<Vec<_>>(),
            "loads": a.loads.len(),
            "transmission_lines": a.lines.len(),
            "plates": a.plates.len(),
            "ground": a.ground.kind,
            "material": a.material,
            "engine": engine_name(a.engine),
            "path": self.path,
        })
    }
}

enum CallError {
    Unknown,
    Failed(String),
}

fn fail<T>(m: String) -> Result<T, CallError> {
    Err(CallError::Failed(m))
}

fn str_arg<'a>(a: &'a Value, k: &str) -> Result<&'a str, CallError> {
    a.get(k).and_then(Value::as_str).ok_or_else(|| CallError::Failed(format!("Missing '{k}'.")))
}

fn num_arg(a: &Value, k: &str) -> Result<f64, CallError> {
    a.get(k).and_then(Value::as_f64).ok_or_else(|| CallError::Failed(format!("Missing '{k}'.")))
}

/// SWR and dB values may be infinite; JSON has no infinity.
fn fin(x: f64) -> Value {
    if x.is_finite() { json!(x) } else { json!("inf") }
}

fn db3(x: f64) -> f64 {
    (mom::db(x) * 1000.0).round() / 1000.0
}

fn engines() -> Value {
    let e = |x: Engine| json!({ "name": engine_name(x), "label": x.label(), "available": crate::nec::engine_available(x) });
    json!([e(Engine::Wiremom), e(Engine::Mininec), e(Engine::Nec2), e(Engine::External)])
}

fn engine_name(e: Engine) -> &'static str {
    crate::settings::engine_key(e)
}

fn parse_engine(s: &str) -> Result<Engine, CallError> {
    let Some(e) = crate::settings::engine_from_key(s) else {
        return fail(format!("Unknown engine '{s}'; use wiremom, mininec, nec2 or external."));
    };
    if !crate::nec::engine_available(e) {
        return fail(unavailable(e).into());
    }
    Ok(e)
}

fn unavailable(e: Engine) -> &'static str {
    if e == Engine::External {
        "No external NEC program is set up: choose it in the program under File → Preferences."
    } else {
        "The NEC-2 engine isn't built into this copy of the program."
    }
}

fn var_kind(s: &str) -> Option<VarKind> {
    Some(match s.to_ascii_lowercase().as_str() {
        "length" => VarKind::Length,
        "move_x" => VarKind::MoveX,
        "move_y" => VarKind::MoveY,
        "move_z" => VarKind::MoveZ,
        "x1" => VarKind::X1,
        "y1" => VarKind::Y1,
        "z1" => VarKind::Z1,
        "x2" => VarKind::X2,
        "y2" => VarKind::Y2,
        "z2" => VarKind::Z2,
        _ => return None,
    })
}

/// Files don't store the engine; keep the one in use.
fn keep_engine(mut a: Antenna, old: &Antenna) -> Antenna {
    a.engine = old.engine;
    a
}

fn checked(a: Antenna) -> Result<Antenna, CallError> {
    if !(a.freq_mhz > 0.0) {
        return fail("The frequency must be above 0 MHz.".into());
    }
    if !crate::nec::engine_available(a.engine) {
        return fail(unavailable(a.engine).into());
    }
    let n = a.wires.len();
    for (i, w) in a.wires.iter().enumerate() {
        if !(w.r.abs() > 0.0) {
            return fail(format!("Wire {i} has no radius."));
        }
        if w.length() <= 0.0 {
            return fail(format!("Wire {i} has no length (p1 and p2 are the same)."));
        }
    }
    let bad = |what: &str, w: usize| fail(format!("A {what} is on wire {w}, but there are only {n} wires (numbered from 0)."));
    if let Some(s) = a.sources.iter().find(|s| s.wire >= n) {
        return bad("source", s.wire);
    }
    if let Some(l) = a.loads.iter().find(|l| l.wire >= n) {
        return bad("load", l.wire);
    }
    if let Some(t) = a.lines.iter().find(|t| t.a_wire >= n || t.b_wire >= n) {
        return bad("transmission line", t.a_wire.max(t.b_wire));
    }
    Ok(a)
}

/// Removes wire `i` with its sources, loads and lines, and renumbers the rest.
fn delete_wire(a: &mut Antenna, i: usize) {
    a.wires.remove(i);
    let shift = |w: &mut usize| if *w > i { *w -= 1 };
    a.sources.retain(|s| s.wire != i);
    a.sources.iter_mut().for_each(|s| shift(&mut s.wire));
    a.loads.retain(|l| l.wire != i);
    a.loads.iter_mut().for_each(|l| shift(&mut l.wire));
    a.lines.retain(|t| t.a_wire != i && t.b_wire != i);
    a.lines.iter_mut().for_each(|t| {
        shift(&mut t.a_wire);
        shift(&mut t.b_wire);
    });
}

/// JSON Merge Patch (RFC 7396): objects merge, null removes, anything else replaces.
fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(p) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let t = target.as_object_mut().unwrap();
    for (k, v) in p {
        if v.is_null() {
            t.remove(k);
        } else {
            merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
        }
    }
}

/// Where the input power goes, in percent of it.
fn power_json(p: &mom::PowerBudget) -> Value {
    let r1 = |x: f64| (x * 1000.0).round() / 10.0;
    let mut v = json!({
        "radiated_pct": r1(p.radiated),
        "radiated_note": mom::RADIATED_NOTE,
        "nec_efficiency_pct": r1(p.nec_efficiency()),
        "nec_efficiency_note": mom::NEC_EFFICIENCY_NOTE,
        "losses_pct": p.losses.iter().map(|(k, x)| (k.to_lowercase(), json!(r1(*x)))).collect::<serde_json::Map<_, _>>(),
    });
    if let Some(g) = p.ground {
        v["ground_pct"] = json!(r1(g));
        v["ground_note"] = json!(if p.reflection_only { mom::REFLECTION_ONLY_NOTE } else { "what the radiated power and the other losses leave, including the ground wave" });
    }
    v
}

fn result_json(r: &CalcResult) -> Value {
    json!({
        "freq_mhz": r.freq_mhz,
        "r_ohm": r.z.re,
        "x_ohm": r.z.im,
        "swr": fin(r.swr),
        "gain_dbi": r.ga_dbi,
        "fb_db": r.fb_db,
        "efficiency_pct": r.eff.is_finite().then(|| (r.eff * 1000.0).round() / 10.0),
        "max_azimuth_deg": r.az_max,
        "max_elevation_deg": r.el_max,
        "polarization": if r.horizontal_pol { "horizontal" } else { "vertical" },
        "ground": r.ground,
        "material": r.material,
        "unknowns": r.n_unknowns,
        "segments": r.n_segs,
        "time_ms": r.t_total_ms,
    })
}

fn ext_is(p: &Path, e: &str) -> bool {
    p.extension().map(|x| x.eq_ignore_ascii_case(e)).unwrap_or(false)
}

/// Reads a .maa, .nec or .ez file as the GUI does.
pub fn open(p: &Path) -> Result<(Antenna, Vec<String>), String> {
    let bytes = std::fs::read(p).map_err(|e| format!("Couldn't open {}: {e}", p.display()))?;
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let r = if ext_is(p, "ez") {
        crate::ez::parse(&bytes, &stem).map(|r| (r.ant, r.warnings))
    } else if ext_is(p, "nec") {
        crate::nec_file::parse(&crate::maa::decode(&bytes), &stem).map(|r| (r.ant, r.warnings))
    } else {
        crate::maa::parse(&crate::maa::decode(&bytes)).map(|a| (a, vec![]))
    };
    r.map_err(|e| format!("Couldn't read {}: {e}", p.display()))
}

/// Writes by extension (.maa when there is none); returns the path written.
pub fn save(ant: &Antenna, p: &Path) -> Result<PathBuf, String> {
    let p = if p.extension().is_none() { p.with_extension("maa") } else { p.to_path_buf() };
    let data = if ext_is(&p, "ez") {
        crate::ez::write(ant)
    } else if ext_is(&p, "nec") {
        crate::nec_file::write(ant).into_bytes()
    } else {
        crate::maa::write(ant).into_bytes()
    };
    std::fs::write(&p, data).map_err(|e| format!("Couldn't save {}: {e}", p.display()))?;
    Ok(p)
}

const INSTRUCTIONS: &str = "wireant models wire antennas with method-of-moments engines. \
The server holds one current antenna. Units: metres, MHz, ohms, dBi. Wires, sources and loads are \
numbered from 0 in JSON (the program's own labels such as w1c number from 1). A source or load sits at \
'pos' along its wire: 0 = p1 end, 0.5 = centre, 1 = p2 end. Use get_antenna to see the full JSON \
before editing it with update_antenna or set_wire.";

fn engine_prop() -> Value {
    json!({ "type": "string", "enum": ["wiremom", "mininec", "nec2", "external"], "description": "Engine for this call only (default: the antenna's engine). external = the user's own NEC-4/NEC-5 program, if set up in the program's Preferences." })
}

fn freq_prop() -> Value {
    json!({ "type": "number", "description": "Frequency in MHz for this call only (default: the antenna's frequency)." })
}

fn tools() -> Value {
    let ro = json!({ "readOnlyHint": true, "openWorldHint": false });
    json!([
        {
            "name": "list_examples",
            "description": "Lists the built-in example antennas and the calculation engines.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": ro,
        },
        {
            "name": "new_antenna",
            "description": "Replaces the current antenna with a built-in example, or with an empty antenna when no example is given.",
            "inputSchema": { "type": "object", "properties": {
                "example": { "type": "integer", "minimum": 0, "description": "Example index from list_examples." },
            } },
        },
        {
            "name": "open_file",
            "description": "Opens an MMANA .maa, NEC .nec or EZNEC .ez file as the current antenna.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string", "description": "Absolute path of the file." },
            }, "required": ["path"] },
        },
        {
            "name": "save_file",
            "description": "Saves the current antenna; the format follows the extension (.maa, .nec or .ez; .maa when there is none). Transmission lines and plates aren't stored in .maa files.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string", "description": "Absolute path; default: the file it was opened from or last saved to." },
            } },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "get_antenna",
            "description": "Returns the current antenna as JSON: wires (p1, p2 in m, radius r in m, seg <= 0 = automatic, insulation ins_t/ins_eps), sources (current: true = a current source, volts then in amps), loads (type rx / lc / rlc), transmission lines, plates, ground (kind free/perfect/real, eps_r, sigma_ms, add_height), material, z0 for SWR, segmentation and engine.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": ro,
        },
        {
            "name": "set_antenna",
            "description": "Replaces the current antenna with the given JSON (the shape get_antenna returns). Missing fields take defaults.",
            "inputSchema": { "type": "object", "properties": {
                "antenna": { "type": "object" },
            }, "required": ["antenna"] },
        },
        {
            "name": "update_antenna",
            "description": "Changes the current antenna with a JSON Merge Patch (RFC 7396) on the get_antenna JSON: objects merge, arrays and values replace. E.g. {\"freq_mhz\": 7.1, \"ground\": {\"kind\": \"real\", \"add_height\": 10}}.",
            "inputSchema": { "type": "object", "properties": {
                "patch": { "type": "object" },
            }, "required": ["patch"] },
        },
        {
            "name": "set_wire",
            "description": "Changes wire 'index' by merging the given fields into it (e.g. {\"p2\": [0, 5.2, 10]}), or adds a wire when index equals the wire count.",
            "inputSchema": { "type": "object", "properties": {
                "index": { "type": "integer", "minimum": 0 },
                "wire": { "type": "object", "properties": {
                    "p1": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                    "p2": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                    "r": { "type": "number", "description": "radius in metres" },
                    "seg": { "type": "integer", "description": "segments; 0 or less = automatic" },
                    "ins_t": { "type": "number", "description": "insulation thickness in metres" },
                    "ins_eps": { "type": "number", "description": "insulation relative permittivity" },
                } },
            }, "required": ["index", "wire"] },
        },
        {
            "name": "delete_wire",
            "description": "Removes wire 'index' with its sources, loads and transmission lines, and renumbers the wires after it.",
            "inputSchema": { "type": "object", "properties": {
                "index": { "type": "integer", "minimum": 0 },
            }, "required": ["index"] },
        },
        {
            "name": "calculate",
            "description": "Calculates the current antenna: feed impedance, SWR against z0, maximum gain and its direction, front-to-back ratio, efficiency and where the input power goes (radiated, lost in wires, loads, lines and the ground). Optionally returns azimuth and elevation pattern cuts through the maximum.",
            "inputSchema": { "type": "object", "properties": {
                "engine": engine_prop(),
                "freq_mhz": freq_prop(),
                "patterns": { "type": "boolean", "description": "Include pattern cuts (default false)." },
                "pattern_step_deg": { "type": "integer", "minimum": 1, "maximum": 90, "description": "Angle step of the cuts (default 5)." },
            } },
            "annotations": ro,
        },
        {
            "name": "sweep",
            "description": "Frequency sweep of the current antenna: R, X and SWR at each frequency, and gain and F/B unless with_gain is false.",
            "inputSchema": { "type": "object", "properties": {
                "f_start_mhz": { "type": "number" },
                "f_stop_mhz": { "type": "number" },
                "steps": { "type": "integer", "minimum": 2, "maximum": 1001, "description": "Number of frequencies (default 21)." },
                "with_gain": { "type": "boolean", "description": "Also calculate gain and F/B (default true; slower)." },
                "engine": engine_prop(),
            }, "required": ["f_start_mhz", "f_stop_mhz"] },
            "annotations": ro,
        },
        {
            "name": "optimize",
            "description": "Adjusts wire geometry to lower a weighted cost of SWR, |X|, gain and F/B at the antenna's frequency (pattern search). Applies the result to the current antenna unless apply is false.",
            "inputSchema": { "type": "object", "properties": {
                "variables": { "type": "array", "items": { "type": "object", "properties": {
                    "wire": { "type": "integer", "minimum": 0 },
                    "kind": { "type": "string", "enum": ["length", "move_x", "move_y", "move_z", "x1", "y1", "z1", "x2", "y2", "z2"],
                              "description": "length changes the wire at its free end (both ends if neither or both are joined to other wires or the ground); move_* shifts the whole wire; x1..z2 move one end. Wire ends joined to other wires move with them." },
                }, "required": ["wire", "kind"] } },
                "goals": { "type": "object", "description": "Weights (default swr 1, reactance 1, gain 0, front_to_back 0).", "properties": {
                    "swr": { "type": "number" },
                    "reactance": { "type": "number" },
                    "gain": { "type": "number" },
                    "front_to_back": { "type": "number" },
                } },
                "max_iterations": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Default 60." },
                "apply": { "type": "boolean", "description": "Replace the current antenna with the result (default true)." },
                "engine": engine_prop(),
            }, "required": ["variables"] },
        },
        {
            "name": "report",
            "description": "Writes the program's PDF antenna report (as Tools → Antenna report in the program): the antenna and its picture, a calculation with its patterns, the resonance finder's results with SWR and impedance plots, and matching networks for the calculated impedance. Calculates and searches for resonances itself; returns the path, the calculation and the resonances found.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string", "description": "Absolute path of the PDF; default: next to the antenna's file, with its name and .pdf." },
                "engine": engine_prop(),
                "freq_mhz": freq_prop(),
                "resonances": { "type": "boolean", "description": "Run the resonance finder (default true)." },
                "res_f_start_mhz": { "type": "number", "description": "Resonance search start (default 15% below the antenna's frequency)." },
                "res_f_stop_mhz": { "type": "number", "description": "Resonance search stop (default 15% above)." },
                "res_points": { "type": "integer", "minimum": 5, "maximum": 1001, "description": "Sweep points for the search (default 31)." },
                "swr_limit": { "type": "number", "description": "SWR for the bandwidth figures (default 2)." },
            } },
            "annotations": { "destructiveHint": true },
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
        s.handle(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap()
    }

    fn tool(s: &mut Server, name: &str, args: Value) -> Value {
        let r = call(s, 9, "tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(r["result"]["isError"], false, "{name}: {r}");
        r["result"]["structuredContent"].clone()
    }

    #[test]
    fn session() {
        let mut s = Server { ant: Antenna::example(0), path: None };
        let init = call(&mut s, 1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(s.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let list = call(&mut s, 2, "tools/list", json!({}));
        assert!(list["result"]["tools"].as_array().unwrap().len() >= 10);

        tool(&mut s, "update_antenna", json!({ "patch": { "material": "lossless" } }));
        let r = tool(&mut s, "calculate", json!({ "engine": "mininec", "patterns": true, "pattern_step_deg": 10 }));
        let rr = r["r_ohm"].as_f64().unwrap();
        assert!(rr > 60.0 && rr < 85.0, "{r}");
        assert_eq!(r["patterns"]["azimuth_cut"]["points"].as_array().unwrap().len(), 37);
        // lossless dipole in free space: all of the input power is radiated
        let eff = r["efficiency_pct"].as_f64().unwrap();
        assert!((eff - 100.0).abs() < 0.3, "{r}");
        assert_eq!(r["power_budget"]["radiated_pct"].as_f64(), Some(eff));
        assert_eq!(r["power_budget"]["losses_pct"]["wires"].as_f64(), Some(0.0));
        assert!(r["power_budget"].get("ground_pct").is_none());
        assert_eq!(r["power_budget"]["nec_efficiency_pct"].as_f64(), Some(100.0));

        // round trip through JSON and a file
        let a = tool(&mut s, "get_antenna", json!({}))["antenna"].clone();
        tool(&mut s, "set_antenna", json!({ "antenna": a }));
        assert_eq!(s.ant, Antenna { material: Material::Lossless, ..Antenna::example(0) });
        let dir = std::env::temp_dir().join(format!("wireant-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("d.maa");
        tool(&mut s, "save_file", json!({ "path": p }));
        tool(&mut s, "new_antenna", json!({ "example": 2 }));
        tool(&mut s, "open_file", json!({ "path": p }));
        assert_eq!(s.ant.wires, Antenna::example(0).wires);

        // the PDF report, next to the antenna's file by default
        let rep = tool(&mut s, "report", json!({ "engine": "mininec", "res_points": 11 }));
        let pdf = dir.join("d.pdf");
        assert_eq!(rep["saved"], json!(pdf));
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        let found = rep["resonances"]["found"].as_array().unwrap();
        assert!(found.iter().any(|r| (r["freq_mhz"].as_f64().unwrap() - 14.0).abs() < 1.5), "{rep}");
        let p2 = dir.join("no_res");
        let rep = tool(&mut s, "report", json!({ "path": p2, "engine": "mininec", "resonances": false }));
        assert_eq!(rep["saved"], json!(dir.join("no_res.pdf")));
        assert!(rep.get("resonances").is_none());
        let _ = std::fs::remove_dir_all(&dir);

        // add a wire, delete it again
        tool(&mut s, "set_wire", json!({ "index": 1, "wire": { "p1": [1, -5, 0], "p2": [1, 5, 0] } }));
        assert_eq!(s.ant.wires.len(), 2);
        tool(&mut s, "delete_wire", json!({ "index": 1 }));
        assert_eq!(s.ant.wires.len(), 1);

        let sw = tool(&mut s, "sweep", json!({ "f_start_mhz": 13.5, "f_stop_mhz": 14.5, "steps": 5, "with_gain": false, "engine": "mininec" }));
        assert_eq!(sw["points"].as_array().unwrap().len(), 5);

        let o = tool(&mut s, "optimize", json!({ "variables": [{ "wire": 0, "kind": "length" }], "engine": "mininec" }));
        assert!(o["end"]["x_ohm"].as_f64().unwrap().abs() < 3.0, "{o}");

        // errors are tool results, not protocol errors
        let bad = call(&mut s, 3, "tools/call", json!({ "name": "set_wire", "arguments": { "index": 0, "wire": { "r": 0 } } }));
        assert_eq!(bad["result"]["isError"], true);
        let unknown = call(&mut s, 4, "tools/call", json!({ "name": "nope", "arguments": {} }));
        assert_eq!(unknown["error"]["code"], -32602);
    }
}
