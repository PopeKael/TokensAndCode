use chrono::{DateTime, Datelike, TimeZone, Utc};
use chrono_tz::Asia::Bangkok;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

const LIMIT: u64 = 272_000;
const FIELDS: [&str; 6] = [
    "input_tokens",
    "cached_input_tokens",
    "cache_write_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
];
const MODELS: [(&str, [f64; 4], [f64; 4]); 3] = [
    (
        "GPT-6 Astra",
        [10.0, 1.0, 12.5, 50.0],
        [20.0, 2.0, 25.0, 75.0],
    ),
    ("GPT-6 Sol", [2.0, 0.1, 2.5, 10.0], [4.0, 0.2, 5.0, 15.0]),
    (
        "GPT-6 Luna",
        [0.1, 0.01, 0.125, 0.5],
        [0.2, 0.02, 0.25, 0.75],
    ),
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
    pub fresh_input_tokens: u64,
    pub response_count: u64,
    pub cost_usd: BTreeMap<String, f64>,
}
impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.cache_write_input_tokens += other.cache_write_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_output_tokens += other.reasoning_output_tokens;
        self.total_tokens += other.total_tokens;
        self.fresh_input_tokens += other.fresh_input_tokens;
        self.response_count += other.response_count;
        for (k, v) in &other.cost_usd {
            *self.cost_usd.entry(k.clone()).or_default() += v;
        }
    }
}
fn token_value(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}
fn usage(v: &Value) -> Usage {
    let input = token_value(v, "input_tokens");
    let cached = token_value(v, "cached_input_tokens").min(input);
    let fresh = input.saturating_sub(cached);
    let cache_write = token_value(v, "cache_write_input_tokens").min(fresh);
    let output = token_value(v, "output_tokens");
    let mut u = Usage {
        input_tokens: input,
        cached_input_tokens: cached,
        cache_write_input_tokens: cache_write,
        output_tokens: output,
        reasoning_output_tokens: token_value(v, "reasoning_output_tokens"),
        total_tokens: token_value(v, "total_tokens"),
        fresh_input_tokens: fresh,
        response_count: 1,
        cost_usd: BTreeMap::new(),
    };
    if u.total_tokens == 0 {
        u.total_tokens = input.saturating_add(output);
    }
    for (name, short, long) in MODELS {
        let r = if input > LIMIT { long } else { short };
        let billable_fresh = fresh.saturating_sub(cache_write);
        let cost = (billable_fresh as f64 * r[0]
            + cached as f64 * r[1]
            + cache_write as f64 * r[2]
            + output as f64 * r[3])
            / 1_000_000.0;
        u.cost_usd.insert(name.to_string(), cost);
    }
    u
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub usage: Usage,
    pub primary_tokens: u64,
    pub subagent_tokens: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Daily {
    pub date: String,
    pub stats: Stats,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NamedStats {
    pub name: String,
    pub stats: Usage,
    pub detail: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Drilldown {
    pub date: String,
    pub project: String,
    pub session_id: String,
    pub source: String,
    pub role: String,
    pub cwd: String,
    pub model: String,
    pub effort: String,
    pub usage: Usage,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AllowanceWindow {
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub observed_at: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Allowances {
    pub plan_type: Option<String>,
    pub five_hour: Option<AllowanceWindow>,
    pub seven_day: Option<AllowanceWindow>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub generated_at: Option<String>,
    pub source_root: String,
    pub source_files_read: usize,
    pub read_errors: Vec<String>,
    pub current_session_id: Option<String>,
    pub current_session: Stats,
    pub today: Stats,
    pub this_week: Stats,
    pub this_month: Stats,
    pub all_time: Stats,
    pub daily_history: Vec<Daily>,
    pub projects: Vec<NamedStats>,
    pub sessions: Vec<NamedStats>,
    pub models: Vec<NamedStats>,
    pub drilldown: Vec<Drilldown>,
    pub allowances: Allowances,
    pub atomic_response_count: usize,
    pub legacy_fallback_records: usize,
    pub duplicate_atomic_response_ids_skipped: usize,
}
impl Snapshot {
    pub fn age_hours(&self) -> Option<i64> {
        let s = self.generated_at.as_ref()?;
        let generated = DateTime::parse_from_rfc3339(s).ok()?.with_timezone(&Utc);
        Some((Utc::now() - generated).num_hours().max(0))
    }
}

#[derive(Clone)]
struct Context {
    ts: Option<DateTime<Utc>>,
    turn: String,
    model: String,
    effort: String,
    cwd: String,
}
#[derive(Clone)]
struct Atomic {
    ts: Option<DateTime<Utc>>,
    response: String,
    turn: String,
    usage: Value,
}
#[derive(Clone)]
struct Cumulative {
    ts: Option<DateTime<Utc>>,
    usage: Value,
}
#[derive(Clone)]
struct Rate {
    ts: Option<DateTime<Utc>>,
    payload: Value,
}
#[derive(Clone, Default)]
struct ThreadLog {
    id: String,
    sid: String,
    parent: String,
    thread_source: String,
    source: String,
    cwd: String,
    meta_ts: Option<DateTime<Utc>>,
    atom: Vec<Atomic>,
    cum: Vec<Cumulative>,
    ctx: Vec<Context>,
}
impl ThreadLog {
    fn role_source(&self) -> (String, String) {
        if self.thread_source == "user"
            || (self.thread_source.is_empty() && self.source == "vscode")
        {
            return ("primary".into(), "Primary".into());
        }
        if self.thread_source == "guardian_review"
            || self.source == "guardian"
            || self.source == "guardian_review"
        {
            return ("subagent".into(), "Guardian".into());
        }
        let label = if self.source.is_empty() {
            "Subagent".into()
        } else {
            self.source
                .replace('_', " ")
                .split_whitespace()
                .map(|w| {
                    let mut c = w.chars();
                    c.next()
                        .map(|x| x.to_uppercase().to_string() + c.as_str())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        ("subagent".into(), label)
    }
}
fn str_at(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}
fn timestamp(v: &Value) -> Option<DateTime<Utc>> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|x| x.with_timezone(&Utc))
}
fn timestamp_epoch(v: &Value) -> Option<String> {
    let n = v.as_i64()?;
    Utc.timestamp_opt(n, 0)
        .single()
        .map(|x| x.with_timezone(&Bangkok).to_rfc3339())
}
fn parse_reader<R: Read>(
    reader: R,
    path: &Path,
    rates: &mut Vec<Rate>,
) -> Result<ThreadLog, String> {
    let mut log = ThreadLog::default();
    let boxed: Box<dyn Read> = if path.extension().is_some_and(|x| x == "zst") {
        Box::new(zstd::stream::read::Decoder::new(reader).map_err(|e| e.to_string())?)
    } else {
        Box::new(reader)
    };
    for line in BufReader::new(boxed).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(row) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let typ = str_at(&row, "type");
        let p = row.get("payload").cloned().unwrap_or(Value::Null);
        let ts = timestamp(&row["timestamp"]);
        match typ.as_str() {
            "session_meta" => {
                log.id = str_at(&p, "id");
                log.sid = str_at(&p, "session_id");
                if log.sid.is_empty() {
                    log.sid = log.id.clone()
                };
                log.parent = str_at(&p, "parent_thread_id");
                log.thread_source = str_at(&p, "thread_source");
                log.source = match p.get("source") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Object(o)) => o
                        .get("subagent")
                        .and_then(|a| a.get("name").or_else(|| a.get("other")))
                        .and_then(Value::as_str)
                        .unwrap_or("subagent")
                        .to_string(),
                    _ => String::new(),
                };
                log.cwd = str_at(&p, "cwd");
                log.meta_ts = timestamp(&p["timestamp"]).or(ts);
            }
            "turn_context" => log.ctx.push(Context {
                ts,
                turn: str_at(&p, "turn_id"),
                model: str_at(&p, "model"),
                effort: str_at(&p, "effort"),
                cwd: str_at(&p, "cwd"),
            }),
            "token_usage_record" => log.atom.push(Atomic {
                ts,
                response: str_at(&p, "response_id"),
                turn: str_at(&p, "turn_id"),
                usage: p.get("usage").cloned().unwrap_or(Value::Null),
            }),
            "event_msg" if str_at(&p, "type") == "token_count" => {
                if let Some(u) = p.get("info").and_then(|x| x.get("total_token_usage")) {
                    log.cum.push(Cumulative {
                        ts,
                        usage: u.clone(),
                    });
                }
                if let Some(r) = p.get("rate_limits").filter(|x| x.is_object()) {
                    rates.push(Rate {
                        ts,
                        payload: r.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    if log.id.is_empty() {
        log.id = path
            .file_stem()
            .and_then(|x| x.to_str())
            .unwrap_or("unknown")
            .to_string()
    };
    if log.sid.is_empty() {
        log.sid = log.id.clone()
    };
    Ok(log)
}
fn count_fields(v: &Value) -> [u64; 6] {
    FIELDS.map(|k| token_value(v, k))
}
fn project(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|x| x.to_str())
        .filter(|x| !x.is_empty())
        .unwrap_or("Unknown project")
        .to_string()
}
fn stats_add(stats: &mut Stats, usage: &Usage, role: &str) {
    stats.usage.add(usage);
    if role == "primary" {
        stats.primary_tokens += usage.total_tokens
    } else {
        stats.subagent_tokens += usage.total_tokens
    }
}
fn insert_named(map: &mut BTreeMap<String, Usage>, key: String, u: &Usage) {
    map.entry(key).or_default().add(u)
}
fn date_string(ts: DateTime<Utc>) -> String {
    ts.with_timezone(&Bangkok).format("%Y-%m-%d").to_string()
}

pub fn scan(root: &Path, mut progress: impl FnMut(usize, usize, &Path)) -> Snapshot {
    let files: Vec<PathBuf> = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("rollout-") && (n.ends_with(".jsonl") || n.ends_with(".jsonl.zst"))
            })
        })
        .collect();
    let mut logs = Vec::new();
    let mut errors = Vec::new();
    let mut rates = Vec::new();
    let total = files.len();
    for (i, p) in files.iter().enumerate() {
        progress(i + 1, total, p);
        let parsed = File::open(p)
            .map_err(|e| e.to_string())
            .and_then(|f| parse_reader(f, p, &mut rates));
        match parsed {
            Ok(x) => logs.push(x),
            Err(e) => errors.push(format!(
                "{}: {}",
                p.file_name().unwrap_or_default().to_string_lossy(),
                e
            )),
        }
    }
    let roots: HashMap<String, String> = logs
        .iter()
        .map(|x| {
            let (r, _) = x.role_source();
            (
                x.id.clone(),
                if r == "primary" {
                    x.id.clone()
                } else {
                    x.sid.clone()
                },
            )
        })
        .collect();
    let id_root: HashMap<String, String> = logs
        .iter()
        .map(|x| {
            let root = roots
                .get(&x.parent)
                .cloned()
                .unwrap_or_else(|| roots.get(&x.sid).cloned().unwrap_or_else(|| x.sid.clone()));
            (x.id.clone(), root)
        })
        .collect();
    let current = logs
        .iter()
        .filter(|x| x.role_source().0 == "primary")
        .max_by_key(|x| x.meta_ts)
        .map(|x| x.id.clone());
    let now = Utc::now();
    let local = now.with_timezone(&Bangkok);
    let today = local.date_naive();
    let week_start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    let month_start = today.with_day(1).unwrap_or(today);
    let mut result = Snapshot {
        generated_at: Some(now.with_timezone(&Bangkok).to_rfc3339()),
        source_root: root.display().to_string(),
        source_files_read: logs.len(),
        read_errors: errors,
        current_session_id: current.clone(),
        ..Snapshot::default()
    };
    let mut days: BTreeMap<String, Stats> = BTreeMap::new();
    let mut projects = BTreeMap::new();
    let mut sessions = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut dims: BTreeMap<(String, String, String, String, String, String, String), Usage> =
        BTreeMap::new();
    let mut seen = HashSet::new();
    let mut rates_plus = Vec::new();
    for rate in rates {
        if str_at(&rate.payload, "plan_type").eq_ignore_ascii_case("plus") {
            rates_plus.push(rate)
        }
    }
    for log in &logs {
        let (role, source) = log.role_source();
        let root_session = id_root
            .get(&log.id)
            .cloned()
            .unwrap_or_else(|| log.sid.clone());
        let cwd_default = log.cwd.clone();
        let mut entries: Vec<(Option<DateTime<Utc>>, Value, String, String, bool)> = Vec::new();
        if !log.atom.is_empty() {
            for atom in &log.atom {
                if !atom.response.is_empty() && !seen.insert(atom.response.clone()) {
                    result.duplicate_atomic_response_ids_skipped += 1;
                    continue;
                }
                entries.push((
                    atom.ts,
                    atom.usage.clone(),
                    atom.response.clone(),
                    atom.turn.clone(),
                    true,
                ));
            }
        } else {
            let mut prev = [0_u64; 6];
            for (i, c) in log.cum.iter().enumerate() {
                let cur = count_fields(&c.usage);
                let delta = std::array::from_fn::<_, 6, _>(|k| cur[k].saturating_sub(prev[k]));
                prev = cur;
                let mut v = serde_json::Map::new();
                for (k, val) in FIELDS.iter().zip(delta) {
                    v.insert((*k).into(), Value::from(val));
                }
                if delta.iter().any(|x| *x > 0) {
                    entries.push((
                        c.ts,
                        Value::Object(v),
                        format!("legacy:{}:{}", log.id, i),
                        String::new(),
                        false,
                    ));
                }
            }
        }
        for (ts, raw, _rid, turn, atomic) in entries {
            let u = usage(&raw);
            let ctx = log
                .ctx
                .iter()
                .filter(|c| c.turn == turn && (c.ts.is_none() || ts.is_none() || c.ts <= ts))
                .max_by_key(|c| c.ts)
                .or_else(|| {
                    log.ctx
                        .iter()
                        .filter(|c| c.turn == turn)
                        .max_by_key(|c| c.ts)
                });
            let model = ctx
                .map(|c| c.model.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Unknown model")
                .to_string();
            let effort = ctx
                .map(|c| c.effort.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Unknown")
                .to_string();
            let cwd = ctx
                .map(|c| c.cwd.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&cwd_default)
                .to_string();
            let proj = project(&cwd);
            let at = ts.unwrap_or(log.meta_ts.unwrap_or(now));
            let ds = date_string(at);
            let n = at.with_timezone(&Bangkok).date_naive();
            let stats = Stats {
                usage: u.clone(),
                primary_tokens: if role == "primary" { u.total_tokens } else { 0 },
                subagent_tokens: if role != "primary" { u.total_tokens } else { 0 },
            };
            if ds == today.to_string() {
                result.today.usage.add(&u);
                result.today.primary_tokens += stats.primary_tokens;
                result.today.subagent_tokens += stats.subagent_tokens;
            }
            if n >= week_start && n <= today {
                result.this_week.usage.add(&u);
                result.this_week.primary_tokens += stats.primary_tokens;
                result.this_week.subagent_tokens += stats.subagent_tokens;
            }
            if n >= month_start && n <= today {
                result.this_month.usage.add(&u);
                result.this_month.primary_tokens += stats.primary_tokens;
                result.this_month.subagent_tokens += stats.subagent_tokens;
            }
            result.all_time.usage.add(&u);
            result.all_time.primary_tokens += stats.primary_tokens;
            result.all_time.subagent_tokens += stats.subagent_tokens;
            if current.as_deref() == Some(&root_session) {
                result.current_session.usage.add(&u);
                result.current_session.primary_tokens += stats.primary_tokens;
                result.current_session.subagent_tokens += stats.subagent_tokens;
            }
            stats_add(days.entry(ds.clone()).or_default(), &u, &role);
            insert_named(&mut projects, proj.clone(), &u);
            insert_named(&mut sessions, root_session.clone(), &u);
            insert_named(&mut models, model.clone(), &u);
            dims.entry((
                ds,
                proj,
                root_session.clone(),
                source.clone(),
                cwd,
                model,
                effort,
            ))
            .or_default()
            .add(&u);
            if atomic {
                result.atomic_response_count += 1
            } else {
                result.legacy_fallback_records += 1
            };
        }
    }
    if let Some(first) = days
        .keys()
        .next()
        .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
    {
        let mut date = first;
        while date <= today {
            days.entry(date.to_string()).or_default();
            date += chrono::Duration::days(1);
        }
    }
    result.daily_history = days
        .into_iter()
        .map(|(date, stats)| Daily { date, stats })
        .collect();
    result.projects = projects
        .into_iter()
        .map(|(name, stats)| NamedStats {
            name,
            stats,
            detail: String::new(),
        })
        .collect();
    result
        .projects
        .sort_by_key(|x| std::cmp::Reverse(x.stats.input_tokens));
    result.sessions = sessions
        .into_iter()
        .map(|(name, stats)| NamedStats {
            name,
            stats,
            detail: String::new(),
        })
        .collect();
    result
        .sessions
        .sort_by_key(|x| std::cmp::Reverse(x.stats.input_tokens));
    result.models = models
        .into_iter()
        .map(|(name, stats)| NamedStats {
            name,
            stats,
            detail: String::new(),
        })
        .collect();
    result
        .models
        .sort_by_key(|x| std::cmp::Reverse(x.stats.input_tokens));
    result.drilldown = dims
        .into_iter()
        .map(
            |((date, project, session_id, source, cwd, model, effort), usage)| Drilldown {
                date,
                project,
                session_id,
                source: source.clone(),
                role: if source == "Primary" {
                    "primary".into()
                } else {
                    "subagent".into()
                },
                cwd,
                model,
                effort,
                usage,
            },
        )
        .collect();
    result.drilldown.sort_by(|a, b| b.date.cmp(&a.date));
    result.allowances = latest_allowances(rates_plus);
    result
}
fn latest_allowances(mut rates: Vec<Rate>) -> Allowances {
    rates.sort_by_key(|r| r.ts);
    fn win(rates: &[Rate], minutes: u64) -> Option<AllowanceWindow> {
        rates.iter().rev().find_map(|r| {
            ["primary", "secondary"]
                .iter()
                .filter_map(|k| r.payload.get(k))
                .find(|x| token_value(x, "window_minutes") == minutes)
                .map(|w| AllowanceWindow {
                    used_percent: w.get("used_percent").and_then(Value::as_f64).unwrap_or(0.0),
                    resets_at: timestamp_epoch(&w["resets_at"]),
                    observed_at: r.ts.map(|x| x.with_timezone(&Bangkok).to_rfc3339()),
                })
        })
    }
    Allowances {
        plan_type: Some("Plus".into()),
        five_hour: win(&rates, 300),
        seven_day: win(&rates, 10080),
    }
}
