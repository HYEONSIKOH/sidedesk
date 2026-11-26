//! Claude Code 기록 읽기.
//! - `~/.claude/sessions/<pid>.json`: 떠 있는 세션(상태 `busy`, `idle` 등)
//! - `~/.claude/projects/<폴더>/<세션>.jsonl`: 한 줄에 기록 하나

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use serde::de::IgnoredAny;
use sidedesk_core::AiTool;
use sidedesk_domain::{Phase, SessionFacts};

use crate::tail::{Tail, contains};
use crate::{clean, folder_name};

/// 이보다 오래 안 바뀐 닫힌 기록은 보지 않는다.
const RECENT: Duration = Duration::from_secs(6 * 3600);
/// 기록 폴더를 다시 훑는 간격
const RESCAN: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
struct LiveFile {
    pid: i32,
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: Option<String>,
    status: Option<String>,
    name: Option<String>,
    #[serde(rename = "nameSource")]
    name_source: Option<String>,
    kind: Option<String>,
}

struct Live {
    cwd: Option<String>,
    status: String,
    /// 사람이 붙인 이름
    name: Option<String>,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    subtype: Option<String>,
    #[serde(rename = "isSidechain", default)]
    sidechain: bool,
    #[serde(rename = "isMeta", default)]
    meta: bool,
    cwd: Option<String>,
    message: Option<Message>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
}

#[derive(Deserialize)]
struct Message {
    stop_reason: Option<String>,
    /// 글자는 남기지 않고 종류만 본다.
    content: Option<Content>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Parts(Vec<Part>),
    Other(IgnoredAny),
}

#[derive(Deserialize)]
struct Part {
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// 기록 끝에서 본 마지막 표시
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Nothing,
    /// 사람 입력, 도구 결과, 쓰는 중인 답
    Busy,
    /// 결과가 아직 없는 도구 호출
    ToolPending,
    TurnEnd,
    /// 사람이 Esc 로 멈춤
    Interrupted,
}

struct Session {
    path: PathBuf,
    tail: Tail,
    cwd: Option<String>,
    custom: Option<String>,
    ai: Option<String>,
    marker: Marker,
}

pub struct Claude {
    root: PathBuf,
    sessions: HashMap<String, Session>,
    scanned: Option<Instant>,
}

impl Claude {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            sessions: HashMap::new(),
            scanned: None,
        }
    }

    pub fn poll(&mut self) -> Vec<SessionFacts> {
        let live = self.live();
        let now = SystemTime::now();
        if self.scanned.is_none_or(|at| at.elapsed() >= RESCAN) {
            self.scanned = Some(Instant::now());
            self.scan(now);
        }
        for id in live.keys() {
            if !self.sessions.contains_key(id)
                && let Some(path) = self.find(id)
            {
                self.add(id.clone(), path);
            }
        }
        let mut facts = Vec::new();
        self.sessions.retain(|id, session| {
            let Ok(lines) = session.tail.read(&session.path) else {
                // 기록이 지워졌다.
                return false;
            };
            for line in lines {
                session.take(&line);
            }
            let live = live.get(id);
            let recent = session
                .tail
                .modified
                .is_some_and(|at| now.duration_since(at).unwrap_or_default() < RECENT);
            if live.is_none() && !recent {
                return false;
            }
            facts.push(session.facts(id, live));
            true
        });
        facts
    }

    /// 떠 있는 대화형 세션. 프로세스가 살아 있는 것만.
    fn live(&self) -> HashMap<String, Live> {
        let Ok(entries) = std::fs::read_dir(self.root.join("sessions")) else {
            return HashMap::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "json"))
            .filter_map(|entry| {
                let text = std::fs::read(entry.path()).ok()?;
                let file: LiveFile = serde_json::from_slice(&text).ok()?;
                if file
                    .kind
                    .as_deref()
                    .is_some_and(|kind| kind != "interactive")
                    || !alive(file.pid)
                {
                    return None;
                }
                let name = file
                    .name
                    .filter(|_| file.name_source.as_deref() == Some("user"));
                Some((
                    file.session_id,
                    Live {
                        cwd: file.cwd,
                        status: file.status.unwrap_or_default(),
                        name,
                    },
                ))
            })
            .collect()
    }

    /// 최근에 바뀐 기록. 하위 작업 폴더로는 내려가지 않는다.
    fn scan(&mut self, now: SystemTime) {
        let Ok(projects) = std::fs::read_dir(self.root.join("projects")) else {
            return;
        };
        let mut found = Vec::new();
        for project in projects.flatten() {
            let Ok(files) = std::fs::read_dir(project.path()) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|e| e != "jsonl") {
                    continue;
                }
                let recent = file
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|at| now.duration_since(at).unwrap_or_default() < RECENT);
                if let (true, Some(id)) = (recent, stem(&path)) {
                    found.push((id, path));
                }
            }
        }
        for (id, path) in found {
            if !self.sessions.contains_key(&id) {
                self.add(id, path);
            }
        }
    }

    fn find(&self, id: &str) -> Option<PathBuf> {
        let name = format!("{id}.jsonl");
        std::fs::read_dir(self.root.join("projects"))
            .ok()?
            .flatten()
            .map(|project| project.path().join(&name))
            .find(|path| path.is_file())
    }

    fn add(&mut self, id: String, path: PathBuf) {
        let mut session = Session {
            path,
            tail: Tail::default(),
            cwd: None,
            custom: None,
            ai: None,
            marker: Marker::Nothing,
        };
        session.scan_titles();
        self.sessions.insert(id, session);
    }
}

impl Session {
    /// 처음 볼 때 파일 전체에서 제목 줄만 찾는다(끝부분만 읽으면 앞쪽 제목을 놓친다).
    fn scan_titles(&mut self) {
        use std::io::BufRead;
        let Ok(file) = std::fs::File::open(&self.path) else {
            return;
        };
        let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
            if contains(&line, b"\"custom-title\"") || contains(&line, b"\"ai-title\"") {
                self.take(&line);
            }
            line.clear();
        }
    }

    fn take(&mut self, raw: &[u8]) {
        let Ok(line) = serde_json::from_slice::<Line>(raw) else {
            return;
        };
        if let Some(title) = line
            .custom_title
            .as_deref()
            .map(clean)
            .filter(|t| !t.is_empty())
        {
            self.custom = Some(title);
        }
        if let Some(title) = line
            .ai_title
            .as_deref()
            .map(clean)
            .filter(|t| !t.is_empty())
        {
            self.ai = Some(title);
        }
        if line.sidechain {
            return;
        }
        if let Some(cwd) = line.cwd {
            self.cwd = Some(cwd);
        }
        let marker = match (line.kind.as_deref(), line.subtype.as_deref()) {
            (Some("user"), _) => {
                // 로컬 명령(/model 등)과 숨은 줄은 답을 부르지 않는다.
                if line.meta
                    || contains(raw, b"<command-name>")
                    || contains(raw, b"<local-command-")
                {
                    return;
                }
                if contains(raw, b"[Request interrupted") {
                    Marker::Interrupted
                } else {
                    Marker::Busy
                }
            }
            (Some("assistant"), _) => {
                let message = line.message.as_ref();
                match message.and_then(|m| m.stop_reason.as_deref()) {
                    Some("tool_use") => Marker::ToolPending,
                    Some(_) => Marker::TurnEnd,
                    // 아직 쓰는 중. 도구를 부르는 중이면 그 표시를 남긴다.
                    None if message.is_some_and(Message::calls_tool) => Marker::ToolPending,
                    None => Marker::Busy,
                }
            }
            (Some("system"), Some("turn_duration")) => Marker::TurnEnd,
            _ => return,
        };
        self.marker = marker;
    }

    fn facts(&self, id: &str, live: Option<&Live>) -> SessionFacts {
        let phase = match live {
            Some(live) => match (live.status.as_str(), self.marker) {
                ("busy", _) => Phase::Working,
                ("waiting" | "blocked", _) => Phase::Waiting,
                (_, Marker::ToolPending) => Phase::Waiting,
                (_, Marker::Interrupted) => Phase::Stopped,
                (_, Marker::Nothing) => Phase::Unknown,
                // 쉬는 중이면 답을 마친 것으로 본다(로컬 명령 뒤 등).
                _ => Phase::TurnEnded,
            },
            None => match self.marker {
                Marker::TurnEnd => Phase::TurnEnded,
                Marker::Nothing => Phase::Unknown,
                _ => Phase::Stopped,
            },
        };
        let cwd = live
            .and_then(|live| live.cwd.as_deref())
            .or(self.cwd.as_deref())
            .unwrap_or("");
        let project = folder_name(cwd);
        let title = self
            .custom
            .clone()
            .or_else(|| live.and_then(|live| live.name.as_deref()).map(clean))
            .or_else(|| self.ai.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| project.clone());
        SessionFacts {
            key: format!("claude:{id}"),
            tool: AiTool::ClaudeCode,
            title,
            project,
            open: live.is_some(),
            phase,
            last_activity: self.tail.modified.unwrap_or(SystemTime::UNIX_EPOCH),
        }
    }
}

impl Message {
    fn calls_tool(&self) -> bool {
        matches!(&self.content, Some(Content::Parts(parts))
            if parts.iter().any(|part| part.kind.as_deref() == Some("tool_use")))
    }
}

fn stem(path: &Path) -> Option<String> {
    path.file_stem()?.to_str().map(str::to_owned)
}

#[cfg(unix)]
fn alive(pid: i32) -> bool {
    // SAFETY: 신호 0 은 보내지 않고 프로세스가 있는지만 본다.
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

#[cfg(not(unix))]
fn alive(_pid: i32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 시험용 가짜 `~/.claude`. 글자는 만든 것뿐이다.
    struct Home {
        root: PathBuf,
    }

    impl Home {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("sidedesk-claude-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("sessions")).unwrap();
            std::fs::create_dir_all(root.join("projects/-work-app")).unwrap();
            Self { root }
        }

        fn log(&self, id: &str) -> PathBuf {
            self.root.join(format!("projects/-work-app/{id}.jsonl"))
        }

        fn write(&self, id: &str, lines: &[&str]) {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.log(id))
                .unwrap();
            for line in lines {
                writeln!(file, "{line}").unwrap();
            }
        }

        fn live(&self, id: &str, status: &str) {
            let pid = std::process::id();
            std::fs::write(
                self.root.join(format!("sessions/{pid}.json")),
                format!(
                    r#"{{"pid":{pid},"sessionId":"{id}","cwd":"/work/app","status":"{status}","kind":"interactive","name":"내 이름","nameSource":"user"}}"#
                ),
            )
            .unwrap();
        }

        fn closed(&self) {
            for entry in std::fs::read_dir(self.root.join("sessions"))
                .unwrap()
                .flatten()
            {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const USER: &str =
        r#"{"type":"user","cwd":"/work/app","message":{"role":"user","content":"하나"}}"#;
    const TOOL: &str = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"text","text":"x"},{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#;
    const RESULT: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"y"}]}}"#;
    const ANSWER: &str = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"끝"}]}}"#;
    const TURN: &str = r#"{"type":"system","subtype":"turn_duration","durationMs":5}"#;
    const ESC: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
    const LOCAL: &str = r#"{"type":"user","message":{"role":"user","content":"<command-name>/model</command-name>"}}"#;
    const META: &str =
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"숨김"}}"#;
    const SIDE: &str = r#"{"type":"assistant","isSidechain":true,"message":{"stop_reason":"tool_use","content":[]}}"#;
    const WRITING: &str = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":null,"content":[{"type":"text","text":"쓰는 중"}]}}"#;

    fn one(claude: &mut Claude) -> SessionFacts {
        let facts = claude.poll();
        assert_eq!(facts.len(), 1, "{facts:?}");
        facts.into_iter().next().unwrap()
    }

    #[test]
    fn live_status_comes_first() {
        let home = Home::new("live");
        home.write("s1", &[USER, TOOL]);
        home.live("s1", "busy");
        let mut claude = Claude::new(home.root.clone());
        let fact = one(&mut claude);
        assert_eq!(fact.phase, Phase::Working);
        assert!(fact.open);
        assert_eq!(fact.project, "app");
        assert_eq!(fact.key, "claude:s1");
        // 쉬는 중인데 도구 결과가 없음 → 허락이나 답을 기다림
        home.live("s1", "idle");
        assert_eq!(one(&mut claude).phase, Phase::Waiting);
        home.live("s1", "waiting");
        assert_eq!(one(&mut claude).phase, Phase::Waiting);
        home.write("s1", &[RESULT, ANSWER, TURN]);
        home.live("s1", "idle");
        assert_eq!(one(&mut claude).phase, Phase::TurnEnded);
    }

    #[test]
    fn esc_local_commands_and_meta() {
        let home = Home::new("esc");
        home.write("s1", &[USER, ESC]);
        home.live("s1", "idle");
        let mut claude = Claude::new(home.root.clone());
        assert_eq!(one(&mut claude).phase, Phase::Stopped);
        // 로컬 명령과 숨은 줄은 단계를 바꾸지 않는다. 쉬는 중이면 답을 마친 것.
        home.write("s1", &[ANSWER, LOCAL, META]);
        assert_eq!(one(&mut claude).phase, Phase::TurnEnded);
        // 하위 작업 줄은 건너뛴다.
        home.write("s1", &[SIDE]);
        assert_eq!(one(&mut claude).phase, Phase::TurnEnded);
    }

    #[test]
    fn closed_sessions() {
        let home = Home::new("closed");
        home.write("s1", &[USER, TOOL]);
        home.live("s1", "busy");
        let mut claude = Claude::new(home.root.clone());
        assert!(one(&mut claude).open);
        // 작업 중에 터미널을 닫음
        home.closed();
        let fact = one(&mut claude);
        assert!(!fact.open);
        assert_eq!(fact.phase, Phase::Stopped);
        home.write("s1", &[RESULT, ANSWER]);
        assert_eq!(one(&mut claude).phase, Phase::TurnEnded);
        // 기록이 지워지면 뺀다.
        std::fs::remove_file(home.log("s1")).unwrap();
        assert!(claude.poll().is_empty());
    }

    #[test]
    fn clear_starts_a_new_session_and_closes_the_old() {
        let home = Home::new("clear");
        home.write("s1", &[USER, ANSWER]);
        home.live("s1", "idle");
        let mut claude = Claude::new(home.root.clone());
        assert!(one(&mut claude).open);
        home.write("s2", &[USER]);
        home.live("s2", "busy");
        let facts = claude.poll();
        let open: Vec<_> = facts
            .iter()
            .filter(|f| f.open)
            .map(|f| f.key.as_str())
            .collect();
        assert_eq!(open, ["claude:s2"]);
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn titles_and_writing() {
        let home = Home::new("titles");
        let mut lines =
            vec![r#"{"type":"ai-title","aiTitle":"자동\n제목","sessionId":"s1"}"#.to_owned()];
        // 제목 줄 뒤로 끝부분보다 많은 기록
        let filler = format!(
            r#"{{"type":"attachment","attachment":"{}"}}"#,
            "x".repeat(1000)
        );
        lines.extend(std::iter::repeat_n(filler, 400));
        lines.push(WRITING.to_owned());
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        home.write("s1", &refs);
        let mut claude = Claude::new(home.root.clone());
        let fact = one(&mut claude);
        assert_eq!(fact.title, "자동 제목");
        assert_eq!(fact.phase, Phase::Stopped);
        // 떠 있으면 사람이 붙인 이름이 자동 제목보다 앞선다.
        home.live("s1", "busy");
        let fact = one(&mut claude);
        assert_eq!(fact.title, "내 이름");
        assert_eq!(fact.phase, Phase::Working);
        home.write(
            "s1",
            &[r#"{"type":"custom-title","customTitle":"고친 이름","sessionId":"s1"}"#],
        );
        assert_eq!(one(&mut claude).title, "고친 이름");
    }

    #[test]
    fn a_huge_line_keeps_the_last_phase() {
        let home = Home::new("huge");
        home.write("s1", &[USER, TOOL]);
        home.live("s1", "idle");
        let mut claude = Claude::new(home.root.clone());
        assert_eq!(one(&mut claude).phase, Phase::Waiting);
        let huge = format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","content":"{}"}}]}}}}"#,
            "y".repeat(2 * 1024 * 1024)
        );
        home.write("s1", &[&huge]);
        // 잘려 온 줄은 못 읽어도 앞 단계를 지킨다.
        assert_eq!(one(&mut claude).phase, Phase::Waiting);
    }

    #[test]
    fn nothing_there_is_empty() {
        let mut claude = Claude::new(PathBuf::from("/nonexistent-sidedesk"));
        assert!(claude.poll().is_empty());
    }
}
