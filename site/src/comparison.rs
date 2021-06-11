//! Functionality for comparing
//! This is mainly used to build the triage report and the perf
//! comparison endpoints

use crate::api;
use crate::db::{self, ArtifactId, Cache, Crate, Profile};
use crate::load::InputData;
use crate::selector::{self, Tag};

use collector::Bound;
use database::Date;
use serde::Serialize;

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

type BoxedError = Box<dyn Error + Send + Sync>;

pub async fn handle_triage(
    body: api::triage::Request,
    data: &InputData,
) -> Result<api::triage::Response, BoxedError> {
    let start = body.start;
    let end = body.end;
    // Compare against self to get next
    let master_commits = rustc_artifacts::master_commits().await?;
    let comparison = compare(
        start.clone(),
        start.clone(),
        "instructions:u".to_owned(),
        data,
        &master_commits,
    )
    .await?;
    let mut after = Bound::Commit(comparison.next(&master_commits).unwrap()); // TODO: handle no next commit

    let mut report = HashMap::new();
    let mut before = start.clone();

    loop {
        let comparison = compare(
            before,
            after.clone(),
            "instructions:u".to_owned(),
            data,
            &master_commits,
        )
        .await?;
        log::info!(
            "Comparing {} to {}",
            comparison.b.commit,
            comparison.a.commit
        );

        // handle results of comparison
        populate_report(&comparison, &mut report).await;

        // Check that there is a next commit and that the
        // after commit is not equal to `end`
        match comparison.next(&master_commits).map(Bound::Commit) {
            Some(next) if Some(&after) != end.as_ref() => {
                before = after;
                after = next;
            }
            _ => break,
        }
    }
    let end = end.unwrap_or(after);

    let report = generate_report(&start, &end, report);
    Ok(api::triage::Response(report))
}

pub async fn handle_compare(
    body: api::days::Request,
    data: &InputData,
) -> Result<api::days::Response, BoxedError> {
    let commits = rustc_artifacts::master_commits().await?;
    let comparison =
        crate::comparison::compare(body.start, body.end, body.stat, data, &commits).await?;

    let conn = data.conn().await;
    let prev = comparison.prev(&commits);
    let next = comparison.next(&commits);
    let is_contiguous = comparison.is_contiguous(&*conn, &commits).await;

    Ok(api::days::Response {
        prev,
        a: comparison.a,
        b: comparison.b,
        next,
        is_contiguous,
    })
}

async fn populate_report(comparison: &Comparison, report: &mut HashMap<Direction, Vec<String>>) {
    if let Some(summary) = summarize_comparison(comparison) {
        if let Some(direction) = summary.direction() {
            let entry = report.entry(direction).or_default();

            entry.push(summary.write(comparison).await)
        }
    }
}

fn summarize_comparison<'a>(comparison: &'a Comparison) -> Option<ComparisonSummary<'a>> {
    let mut benchmarks = comparison.get_benchmarks();
    // Skip empty commits, sometimes happens if there's a compiler bug or so.
    if benchmarks.len() == 0 {
        return None;
    }

    let cmp = |b1: &BenchmarkComparison, b2: &BenchmarkComparison| {
        b1.log_change()
            .partial_cmp(&b2.log_change())
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    let lo = benchmarks
        .iter()
        .enumerate()
        .min_by(|&(_, b1), &(_, b2)| cmp(b1, b2))
        .filter(|(_, c)| c.is_significant() && !c.is_increase())
        .map(|(i, _)| i);
    let lo = lo.map(|lo| benchmarks.remove(lo));
    let hi = benchmarks
        .iter()
        .enumerate()
        .max_by(|&(_, b1), &(_, b2)| cmp(b1, b2))
        .filter(|(_, c)| c.is_significant() && c.is_increase())
        .map(|(i, _)| i);
    let hi = hi.map(|hi| benchmarks.remove(hi));

    Some(ComparisonSummary { hi, lo })
}

struct ComparisonSummary<'a> {
    hi: Option<BenchmarkComparison<'a>>,
    lo: Option<BenchmarkComparison<'a>>,
}

impl ComparisonSummary<'_> {
    /// The direction of the changes
    fn direction(&self) -> Option<Direction> {
        let d = match (&self.hi, &self.lo) {
            (None, None) => return None,
            (Some(b), None) => b.direction(),
            (None, Some(b)) => b.direction(),
            (Some(a), Some(b)) if a.is_increase() == b.is_increase() => a.direction(),
            _ => Direction::Mixed,
        };
        Some(d)
    }

    /// The changes ordered by their signficance (most significant first)
    fn ordered_changes(&self) -> Vec<&BenchmarkComparison<'_>> {
        match (&self.hi, &self.lo) {
            (None, None) => Vec::new(),
            (Some(b), None) => vec![b],
            (None, Some(b)) => vec![b],
            (Some(a), Some(b))
                if b.log_change()
                    .abs()
                    .partial_cmp(&a.log_change().abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
                    == std::cmp::Ordering::Greater =>
            {
                vec![b, a]
            }
            (Some(a), Some(b)) => vec![a, b],
        }
    }

    async fn write(&self, comparison: &Comparison) -> String {
        use std::fmt::Write;

        let mut result = if let Some(pr) = comparison.b.pr {
            let title = gh_pr_title(pr).await;
            format!(
                "{} [#{}](https://github.com/rust-lang/rust/issues/{})\n",
                title, pr, pr
            )
        } else {
            String::from("<Unknown Change>\n")
        };
        let start = &comparison.a.commit;
        let end = &comparison.b.commit;
        let link = &compare_link(start, end);

        for change in self.ordered_changes() {
            write!(result, "- ").unwrap();
            change.summary_line(&mut result, link)
        }
        result
    }
}

/// Compare two bounds on a given stat
pub async fn compare(
    start: Bound,
    end: Bound,
    stat: String,
    data: &InputData,
    master_commits: &[rustc_artifacts::Commit],
) -> Result<Comparison, BoxedError> {
    let a = data
        .data_for(true, start.clone())
        .ok_or(format!("could not find start commit for bound {:?}", start))?;
    let b = data
        .data_for(false, end.clone())
        .ok_or(format!("could not find end commit for bound {:?}", end))?;
    let cids = Arc::new(vec![a.clone().into(), b.clone().into()]);

    let query = selector::Query::new()
        .set::<String>(Tag::Crate, selector::Selector::All)
        .set::<String>(Tag::Cache, selector::Selector::All)
        .set::<String>(Tag::Profile, selector::Selector::All)
        .set(Tag::ProcessStatistic, selector::Selector::One(stat.clone()));

    let mut responses = data.query::<Option<f64>>(query, cids).await?;

    let conn = data.conn().await;

    Ok(Comparison {
        a: DateData::consume_one(&*conn, a.clone(), &mut responses, master_commits).await,
        a_id: a,
        b: DateData::consume_one(&*conn, b.clone(), &mut responses, master_commits).await,
        b_id: b,
    })
}

/// Data associated with a specific date
#[derive(Debug, Clone, Serialize)]
pub struct DateData {
    pub date: Option<Date>,
    pub pr: Option<u32>,
    pub commit: String,
    pub data: HashMap<String, Vec<(String, f64)>>,
    // crate -> nanoseconds
    pub bootstrap: HashMap<String, u64>,
}

impl DateData {
    async fn consume_one<'a, T>(
        conn: &dyn database::Connection,
        commit: ArtifactId,
        series: &mut [selector::SeriesResponse<T>],
        master_commits: &[rustc_artifacts::Commit],
    ) -> Self
    where
        T: Iterator<Item = (db::ArtifactId, Option<f64>)>,
    {
        let mut data = HashMap::new();

        for response in series {
            let (id, point) = response.series.next().expect("must have element");
            assert_eq!(commit, id);

            let point = if let Some(pt) = point {
                pt
            } else {
                continue;
            };
            data.entry(format!(
                "{}-{}",
                response.path.get::<Crate>().unwrap(),
                response.path.get::<Profile>().unwrap(),
            ))
            .or_insert_with(Vec::new)
            .push((response.path.get::<Cache>().unwrap().to_string(), point));
        }

        let bootstrap = conn.get_bootstrap(&[conn.artifact_id(&commit).await]).await;
        let bootstrap = bootstrap
            .into_iter()
            .filter_map(|(k, mut v)| {
                v.pop()
                    .unwrap_or_default()
                    // FIXME: if we're hovering right at the 1 second mark,
                    // this might mean we end up with a Some for one commit and
                    // a None for the other commit. Ultimately it doesn't matter
                    // that much -- we'll mostly just ignore such results.
                    // Anything less than a second in wall-time measurements is
                    // always going to be pretty high variance just from process
                    // startup overheads and such, though, so we definitely
                    // don't want to compare those values.
                    .filter(|v| v.as_secs() >= 1)
                    .map(|v| (k, v.as_nanos() as u64))
            })
            .collect::<HashMap<_, _>>();

        Self {
            date: if let ArtifactId::Commit(c) = &commit {
                Some(c.date)
            } else {
                None
            },
            pr: if let ArtifactId::Commit(c) = &commit {
                if let Some(m) = master_commits.iter().find(|m| m.sha == c.sha) {
                    m.pr
                } else {
                    conn.pr_of(&c.sha).await
                }
            } else {
                None
            },
            commit: match commit {
                ArtifactId::Commit(c) => c.sha,
                ArtifactId::Artifact(i) => i,
            },
            data,
            bootstrap,
        }
    }
}

// A comparison of two artifacts
pub struct Comparison {
    pub a_id: ArtifactId,
    pub a: DateData,
    pub b_id: ArtifactId,
    pub b: DateData,
}

impl Comparison {
    /// Gets the previous commit before `a`
    pub fn prev(&self, master_commits: &[rustc_artifacts::Commit]) -> Option<String> {
        match &self.a_id {
            ArtifactId::Commit(a) => master_commits
                .iter()
                .find(|c| c.sha == a.sha)
                .map(|c| c.parent_sha.clone()),
            ArtifactId::Artifact(_) => None,
        }
    }

    /// Determines if `a` and `b` are contiguous
    pub async fn is_contiguous(
        &self,
        conn: &dyn database::Connection,
        master_commits: &[rustc_artifacts::Commit],
    ) -> bool {
        match (&self.a_id, &self.b_id) {
            (ArtifactId::Commit(a), ArtifactId::Commit(b)) => {
                if let Some(b) = master_commits.iter().find(|c| c.sha == b.sha) {
                    b.parent_sha == a.sha
                } else {
                    conn.parent_of(&b.sha).await.map_or(false, |p| p == a.sha)
                }
            }
            _ => false,
        }
    }

    /// Gets the sha of the next commit after `b`
    pub fn next(&self, master_commits: &[rustc_artifacts::Commit]) -> Option<String> {
        match &self.b_id {
            ArtifactId::Commit(b) => master_commits
                .iter()
                .find(|c| c.parent_sha == b.sha)
                .map(|c| c.sha.clone()),
            ArtifactId::Artifact(_) => None,
        }
    }

    fn get_benchmarks<'a>(&'a self) -> Vec<BenchmarkComparison<'a>> {
        let mut result = Vec::new();
        for (bench_name, a) in self.a.data.iter() {
            if bench_name.ends_with("-doc") {
                continue;
            }
            if let Some(b) = self.b.data.get(bench_name) {
                for (cache_state, a) in a.iter() {
                    if let Some(b) = b.iter().find(|(cs, _)| cs == cache_state).map(|(_, b)| b) {
                        result.push(BenchmarkComparison {
                            bench_name,
                            cache_state,
                            results: (a.clone(), b.clone()),
                        })
                    }
                }
            }
        }

        result
    }
}

// A single comparison based on benchmark and cache state
#[derive(Debug)]
struct BenchmarkComparison<'a> {
    bench_name: &'a str,
    cache_state: &'a str,
    results: (f64, f64),
}

const SIGNIFICANCE_THRESHOLD: f64 = 0.01;
impl BenchmarkComparison<'_> {
    fn log_change(&self) -> f64 {
        let (a, b) = self.results;
        (b / a).ln()
    }

    fn is_increase(&self) -> bool {
        let (a, b) = self.results;
        b > a
    }

    fn is_significant(&self) -> bool {
        // This particular (benchmark, cache) combination frequently varies
        if self.bench_name.starts_with("coercions-debug")
            && self.cache_state == "incr-patched: println"
        {
            self.relative_change().abs() > 2.0
        } else {
            self.log_change().abs() > SIGNIFICANCE_THRESHOLD
        }
    }

    fn relative_change(&self) -> f64 {
        let (a, b) = self.results;
        (b - a) / a
    }

    fn direction(&self) -> Direction {
        if self.log_change() > 0.0 {
            Direction::Regression
        } else {
            Direction::Improvement
        }
    }

    fn summary_line(&self, summary: &mut String, link: &str) {
        use std::fmt::Write;
        let magnitude = self.log_change().abs();
        let size = if magnitude > 0.10 {
            "Very large"
        } else if magnitude > 0.05 {
            "Large"
        } else if magnitude > 0.01 {
            "Moderate"
        } else if magnitude > 0.005 {
            "Small"
        } else {
            "Very small"
        };

        let percent = self.relative_change() * 100.0;
        write!(
            summary,
            "{} {} in [instruction counts]({})",
            size,
            self.direction(),
            link
        )
        .unwrap();
        writeln!(
            summary,
            " (up to {:.1}% on `{}` builds of `{}`)",
            percent, self.cache_state, self.bench_name
        )
        .unwrap();
    }
}

// The direction of a performance change
#[derive(PartialEq, Eq, Hash)]
enum Direction {
    Improvement,
    Regression,
    Mixed,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let description = match self {
            Direction::Improvement => "improvement",
            Direction::Regression => "regression",
            Direction::Mixed => "mixed",
        };
        write!(f, "{}", description)
    }
}

fn generate_report(
    start: &Bound,
    end: &Bound,
    mut report: HashMap<Direction, Vec<String>>,
) -> String {
    fn fmt_bound(bound: &Bound) -> String {
        match bound {
            Bound::Commit(s) => s.to_owned(),
            Bound::Date(s) => s.format("%Y-%m-%d").to_string(),
            _ => "???".to_owned(),
        }
    }
    let start = fmt_bound(start);
    let end = fmt_bound(end);
    let regressions = report.remove(&Direction::Regression).unwrap_or_default();
    let improvements = report.remove(&Direction::Improvement).unwrap_or_default();
    let mixed = report.remove(&Direction::Mixed).unwrap_or_default();
    format!(
        r#####"# {date} Triage Log

TODO: Summary

Triage done by **@???**.
Revision range: [{first_commit}..{last_commit}](https://perf.rust-lang.org/?start={first_commit}&end={last_commit}&absolute=false&stat=instructions%3Au)

{num_regressions} Regressions, {num_improvements} Improvements, {num_mixed} Mixed
??? of them in rollups

#### Regressions

{regressions}

#### Improvements

{improvements}

#### Mixed

{mixed}

#### Nags requiring follow up

TODO: Nags

"#####,
        date = chrono::Utc::today().format("%Y-%m-%d"),
        first_commit = start,
        last_commit = end,
        num_regressions = regressions.len(),
        num_improvements = improvements.len(),
        num_mixed = mixed.len(),
        regressions = regressions.join("\n\n"),
        improvements = improvements.join("\n\n"),
        mixed = mixed.join("\n\n"),
    )
}

fn compare_link(start: &str, end: &str) -> String {
    format!(
        "https://perf.rust-lang.org/compare.html?start={}&end={}&stat=instructions:u",
        start, end
    )
}

async fn gh_pr_title(pr: u32) -> String {
    let url = format!("https://api.github.com/repos/rust-lang/rust/pulls/{}", pr);
    let client = reqwest::Client::new();
    let mut request = client
        .get(&url)
        .header("Content-Type", "application/json")
        .header("User-Agent", "rustc-perf");

    if let Some(token) = std::env::var("GITHUB_TOKEN").ok() {
        request = request.header("Authorization", format!("token {}", token));
    }

    async fn send(request: reqwest::RequestBuilder) -> Result<String, BoxedError> {
        Ok(request
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?
            .get("title")
            .ok_or_else(|| "JSON was malformed".to_owned())?
            .as_str()
            .ok_or_else(|| "JSON was malformed".to_owned())?
            .to_owned())
    }
    match send(request).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error fetching url: {}", e);
            String::from("<UNKNOWN>")
        }
    }
}