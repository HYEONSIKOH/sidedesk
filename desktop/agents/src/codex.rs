//! Codex 기록 읽기.
//! - `~/.codex/sessions/YYYY/MM/DD/rollout-<시각>-<id>.jsonl`: 한 줄에 `{timestamp, type, payload}`
//! - `~/.codex/session_index.jsonl`: `{id, thread_name, updated_at}`
//!
//! 떠 있는지 알려 주는 파일이 없어 마지막 기록이 30분 안이면 떠 있는 것으로 본다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use sidedesk_core::AiTool;
use sidedesk_domain::{Phase, SessionFacts};

use crate::tail::Tail;
use crate::{clean, folder_name};

const RECENT: Duration = Duration::from_secs(6 * 3600);
const OPEN: Duration = Duration::from_secs(30 * 60);
const RESCAN: Duration = Duration::from_secs(30);
/// 날짜 폴더를 며칠 앞까지 훑는지(이어서 연 세션은 처음 날짜 폴더에 덧붙는다)
const DAYS: u64 = 14;
/// 첫 줄(session_meta)을 읽는 상한
const FIRST_LINE: u64 = 1024 * 1024;
const WAITS: [&str; 4] = [
    "exec_approval_request",
    "apply_patch_approval_request",
    "request_user_input",
    "elicitation_request",
];

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(rename = "type")]
    kind: Option<String>,
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct Index {
    id: String,
    thread_name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Nothing,
    Working,
    Waiting,
    TurnEnd,
    Aborted,
}

struct Session {
    path: PathBuf,
    tail: Tail,
    cwd: Option<String>,
    marker: Marker,
}

pub struct Codex {
    root: PathBuf,
    sessions: HashMap<String, Session>,
    names: HashMap<String, String>,
    index_modified: Option<SystemTime>,
    scanned: Option<Instant>,
}

impl Codex {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            sessions: HashMap::new(),
            names: HashMap::new(),
            index_modified: None,
            scanned: None,
        }
    }

    pub fn poll(&mut self) -> Vec<SessionFacts> {
        let now = SystemTime::now();
        if self.scanned.is_none_or(|at| at.elapsed() >= RESCAN) {
            self.scanned = Some(Instant::now());
            self.scan(now);
        }
        self.read_index();
        let mut facts = Vec::new();
        let names = &self.names;
        self.sessions.retain(|id, session| {
            let Ok(lines) = session.tail.read(&session.path) else {
                return false;
            };
            for line in lines {
                session.take(&line);
            }
            let age = session.tail.modified.map_or(Duration::MAX, |at| {
                now.duration_since(at).unwrap_or_default()
            });
            if age >= RECENT {
                return false;
            }
            facts.push(session.facts(id, names.get(id), age < OPEN));
            true
        });
        facts
    }

    /// 최근 날짜 폴더에서 최근에 바뀐 기록을 찾는다.
    fn scan(&mut self, now: SystemTime) {
        let base = self.root.join("sessions");
        let today = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() / 86_400);
        // Codex 는 폴더 날짜를 현지 시각으로 쓴다. UTC 날짜보다 하루 앞(UTC+14 까지)부터 본다.
        for back in 0..=DAYS {
            let (y, m, d) = civil((today + 1).saturating_sub(back) as i64);
            let folder = base.join(format!("{y:04}/{m:02}/{d:02}"));
            let Ok(files) = std::fs::read_dir(&folder) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                let Some(id) = rollout_id(&path) else {
                    continue;
                };
                if self.sessions.contains_key(&id) {
                    continue;
                }
                let recent = file
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|at| now.duration_since(at).unwrap_or_default() < RECENT);
                if recent {
                    let cwd = first_cwd(&path);
                    self.sessions.insert(
                        id,
                        Session {
                            path,
                            tail: Tail::default(),
                            cwd,
                            marker: Marker::Nothing,
                        },
                    );
                }
            }
        }
    }

    /// 세션 이름. 파일이 바뀌었을 때만 다시 읽는다.
    fn read_index(&mut self) {
        let path = self.root.join("session_index.jsonl");
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if modified.is_none() || modified == self.index_modified {
            return;
        }
        self.index_modified = modified;
        let Ok(text) = std::fs::read(&path) else {
            return;
        };
        self.names = text
            .split(|b| *b == b'\n')
            .filter_map(|line| serde_json::from_slice::<Index>(line).ok())
            .filter_map(|index| Some((index.id, clean(&index.thread_name?))))
            .filter(|(_, name)| !name.is_empty())
            .collect();
    }
}

impl Session {
    fn take(&mut self, raw: &[u8]) {
        let Ok(line) = serde_json::from_slice::<Line>(raw) else {
            return;
        };
        let Some(payload) = line.payload else {
            return;
        };
        if line.kind.as_deref() == Some("session_meta") {
            if let Some(cwd) = payload.cwd {
                self.cwd = Some(cwd);
            }
            return;
        }
        let kind = payload.kind.as_deref().unwrap_or("");
        let marker = match (line.kind.as_deref(), kind) {
            (Some("event_msg"), "task_started") => Marker::Working,
            (Some("event_msg"), "task_complete") => Marker::TurnEnd,
            (Some("event_msg"), "turn_aborted") => Marker::Aborted,
            (Some("event_msg"), kind) if WAITS.contains(&kind) => Marker::Waiting,
            // 도구 결과가 오면 허락이 끝나 다시 일한다.
            (Some("response_item"), "function_call_output" | "custom_tool_call_output")
                if self.marker == Marker::Waiting =>
            {
                Marker::Working
            }
            _ => return,
        };
        self.marker = marker;
    }

    fn facts(&self, id: &str, name: Option<&String>, open: bool) -> SessionFacts {
        let phase = match self.marker {
            Marker::Nothing => Phase::Unknown,
            Marker::Working => Phase::Working,
            Marker::Waiting => Phase::Waiting,
            Marker::TurnEnd => Phase::TurnEnded,
            Marker::Aborted => Phase::Stopped,
        };
        let project = folder_name(self.cwd.as_deref().unwrap_or(""));
        let title = name
            .cloned()
            .filter(|title| !title.is_empty())
            .or_else(|| Some(project.clone()).filter(|p| !p.is_empty()))
            .unwrap_or_else(|| "Codex".to_owned());
        SessionFacts {
            key: format!("codex:{id}"),
            tool: AiTool::Codex,
            title,
            project,
            open,
            phase,
            last_activity: self.tail.modified.unwrap_or(SystemTime::UNIX_EPOCH),
        }
    }
}

/// `rollout-2026-11-25T15-45-50-<id>.jsonl` → id(뒤 다섯 덩이의 UUID)
fn rollout_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let parts: Vec<&str> = stem.split('-').collect();
    (parts.len() >= 5).then(|| parts[parts.len() - 5..].join("-"))
}

/// 첫 줄의 작업 폴더. 첫 줄은 지시문이 들어 커질 수 있어 상한까지만 읽는다.
fn first_cwd(path: &Path) -> Option<String> {
    use std::io::{BufRead, Read};
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file.take(FIRST_LINE));
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).ok()?;
    if let Ok(parsed) = serde_json::from_slice::<Line>(&line) {
        return (parsed.kind.as_deref() == Some("session_meta"))
            .then_some(parsed.payload?.cwd)
            .flatten();
    }
    // 첫 줄이 상한보다 길어 잘렸으면 작업 폴더 글자만 찾는다.
    let at = line.windows(7).position(|w| w == b"\"cwd\":\"")? + 7;
    let end = line[at..].iter().position(|b| *b == b'"')? + at;
    String::from_utf8(line[at..end].to_vec()).ok()
}

/// 1970-01-01 부터 날 수 → (년, 월, 일)
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Home {
        root: PathBuf,
        folder: PathBuf,
    }

    const ID: &str = "01a21174-18da-7601-ab85-e133f3ea3fcd";

    impl Home {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("sidedesk-codex-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let today = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                / 86_400;
            let (y, m, d) = civil(today as i64 - 3);
            let folder = root.join(format!("sessions/{y:04}/{m:02}/{d:02}"));
            std::fs::create_dir_all(&folder).unwrap();
            Self { root, folder }
        }

        fn log(&self) -> PathBuf {
            self.folder
                .join(format!("rollout-2026-11-25T15-45-50-{ID}.jsonl"))
        }

        fn write(&self, lines: &[&str]) {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.log())
                .unwrap();
            for line in lines {
                writeln!(file, "{line}").unwrap();
            }
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const META: &str = r#"{"timestamp":"t","type":"session_meta","payload":{"id":"x","cwd":"/work/tool","base_instructions":"긴 지시"}}"#;
    const START: &str = r#"{"timestamp":"t","type":"event_msg","payload":{"type":"task_started"}}"#;
    const DONE: &str = r#"{"timestamp":"t","type":"event_msg","payload":{"type":"task_complete"}}"#;
    const ASK: &str =
        r#"{"timestamp":"t","type":"event_msg","payload":{"type":"exec_approval_request"}}"#;
    const OUTPUT: &str =
        r#"{"timestamp":"t","type":"response_item","payload":{"type":"function_call_output"}}"#;
    const ABORT: &str = r#"{"timestamp":"t","type":"event_msg","payload":{"type":"turn_aborted"}}"#;

    fn one(codex: &mut Codex) -> SessionFacts {
        let facts = codex.poll();
        assert_eq!(facts.len(), 1, "{facts:?}");
        facts.into_iter().next().unwrap()
    }

    #[test]
    fn turns_approvals_and_aborts() {
        let home = Home::new("turns");
        home.write(&[META, START]);
        let mut codex = Codex::new(home.root.clone());
        let fact = one(&mut codex);
        assert_eq!(fact.phase, Phase::Working);
        assert_eq!(fact.key, format!("codex:{ID}"));
        assert_eq!(fact.project, "tool");
        assert_eq!(fact.title, "tool");
        assert!(fact.open);
        home.write(&[ASK]);
        assert_eq!(one(&mut codex).phase, Phase::Waiting);
        home.write(&[OUTPUT]);
        assert_eq!(one(&mut codex).phase, Phase::Working);
        home.write(&[DONE]);
        assert_eq!(one(&mut codex).phase, Phase::TurnEnded);
        home.write(&[START, ABORT]);
        assert_eq!(one(&mut codex).phase, Phase::Stopped);
    }

    #[test]
    fn a_folder_dated_ahead_of_utc_is_seen() {
        let root =
            std::env::temp_dir().join(format!("sidedesk-codex-ahead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let today = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 86_400;
        // 한국 새벽처럼 현지 날짜가 UTC 보다 하루 앞선 폴더
        let (y, m, d) = civil(today as i64 + 1);
        let folder = root.join(format!("sessions/{y:04}/{m:02}/{d:02}"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(format!("rollout-2026-11-28T03-00-00-{ID}.jsonl")),
            format!("{META}\n{START}\n"),
        )
        .unwrap();
        let mut codex = Codex::new(root.clone());
        assert_eq!(one(&mut codex).phase, Phase::Working);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_huge_first_line_still_gives_the_folder() {
        let home = Home::new("huge");
        let meta = format!(
            r#"{{"timestamp":"t","type":"session_meta","payload":{{"cwd":"/work/tool","base_instructions":"{}"}}}}"#,
            "x".repeat(2 * 1024 * 1024)
        );
        home.write(&[&meta, START]);
        let mut codex = Codex::new(home.root.clone());
        assert_eq!(one(&mut codex).project, "tool");
        assert_eq!(rollout_id(&home.log()).as_deref(), Some(ID));
    }

    #[test]
    fn no_folder_falls_back_to_the_tool_name() {
        let home = Home::new("nameless");
        home.write(&[START]);
        let mut codex = Codex::new(home.root.clone());
        assert_eq!(one(&mut codex).title, "Codex");
    }

    #[test]
    fn names_come_from_the_index() {
        let home = Home::new("names");
        home.write(&[META, START]);
        std::fs::write(
            home.root.join("session_index.jsonl"),
            format!("{{\"id\":\"{ID}\",\"thread_name\":\"이름\\n붙임\",\"updated_at\":\"t\"}}\n"),
        )
        .unwrap();
        let mut codex = Codex::new(home.root.clone());
        assert_eq!(one(&mut codex).title, "이름 붙임");
        std::fs::remove_file(home.log()).unwrap();
        assert!(codex.poll().is_empty());
    }

    #[test]
    fn ids_from_file_names() {
        assert_eq!(
            rollout_id(Path::new(&format!(
                "/x/rollout-2026-11-25T15-45-50-{ID}.jsonl"
            ))),
            Some(ID.to_owned())
        );
        assert_eq!(rollout_id(Path::new("/x/other.jsonl")), None);
        assert_eq!(civil(20_419), (2025, 11, 27));
    }
}
