//! GitHub, GitLab 수. 설치된 `gh`, `glab` 을 정해진 인자로 돌려 숫자만 쓴다.
//! 토큰을 읽거나 남기지 않고(환경 변수는 그대로 물려받음), 출력 글자를 오류 문장에 넣지 않는다.

mod run;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sidedesk_core::{Forge, ForgeCounts, ForgeState};
use sidedesk_domain::Forges;

pub use run::{Ran, Runner, System, locate};

/// 숫자 상한(GitHub 알림은 한 쪽 50개까지만 센다)
pub const NOTICE_CAP: u32 = 50;
const GITHUB_QUERY: &str = "query { r: search(type: ISSUE, query: \"is:open is:pr archived:false review-requested:@me\") { issueCount } m: search(type: ISSUE, query: \"is:open is:pr archived:false author:@me\") { issueCount } }";
const GITLAB_QUERY: &str = "query { currentUser { reviewRequestedMergeRequests(state: opened) { count } authoredMergeRequests(state: opened) { count } todos(state: pending) { count } } }";

pub struct Forgery {
    runner: Arc<dyn Runner>,
    gh: Option<PathBuf>,
    glab: Option<PathBuf>,
}

impl Forgery {
    /// 맥 앱에서 쓴다. 20초 넘는 실행은 끝낸다.
    pub fn system(home: PathBuf) -> Self {
        let gh = locate("gh", &home);
        let glab = locate("glab", &home);
        Self::with(
            Arc::new(System::new(home, Duration::from_secs(20))),
            gh,
            glab,
        )
    }

    pub fn with(runner: Arc<dyn Runner>, gh: Option<PathBuf>, glab: Option<PathBuf>) -> Self {
        Self { runner, gh, glab }
    }

    fn github(&self) -> ForgeCounts {
        let Some(gh) = &self.gh else {
            return state(ForgeState::Missing);
        };
        let search = self.runner.run(
            gh,
            &[
                "api",
                "graphql",
                "--hostname",
                "github.com",
                "-f",
                &format!("query={GITHUB_QUERY}"),
                "--jq",
                "\"\\(.data.r.issueCount) \\(.data.m.issueCount)\"",
            ],
        );
        if let Some(problem) = problem(&search) {
            return state(problem);
        }
        let [reviews, mine] = numbers(&search.stdout)[..] else {
            return state(ForgeState::Failed);
        };
        let notices = self.runner.run(
            gh,
            &[
                "api",
                "notifications?per_page=50",
                "--hostname",
                "github.com",
                "--jq",
                "length",
            ],
        );
        if let Some(problem) = problem(&notices) {
            return state(problem);
        }
        let [notices] = numbers(&notices.stdout)[..] else {
            return state(ForgeState::Failed);
        };
        ForgeCounts {
            state: ForgeState::Ok as i32,
            reviews,
            mine,
            notices: notices.min(NOTICE_CAP),
        }
    }

    fn gitlab(&self) -> ForgeCounts {
        let Some(glab) = &self.glab else {
            return state(ForgeState::Missing);
        };
        let ran = self.runner.run(
            glab,
            &["api", "graphql", "-f", &format!("query={GITLAB_QUERY}")],
        );
        if let Some(problem) = problem(&ran) {
            return state(problem);
        }
        let Ok(json) = serde_json::from_slice::<Value>(&ran.stdout) else {
            return state(ForgeState::Failed);
        };
        if json["data"].is_null() {
            return state(ForgeState::Failed);
        }
        let user = &json["data"]["currentUser"];
        if user.is_null() {
            return state(ForgeState::SignedOut);
        }
        let count = |field: &str| {
            user[field]["count"]
                .as_u64()
                .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
        };
        match (
            count("reviewRequestedMergeRequests"),
            count("authoredMergeRequests"),
            count("todos"),
        ) {
            (Some(reviews), Some(mine), Some(notices)) => ForgeCounts {
                state: ForgeState::Ok as i32,
                reviews,
                mine,
                notices,
            },
            _ => state(ForgeState::Failed),
        }
    }
}

impl Forges for Forgery {
    fn counts(&self) -> Forge {
        Forge {
            github: Some(self.github()),
            gitlab: Some(self.gitlab()),
        }
    }

    fn stop(&self) {
        self.runner.stop();
    }
}

fn state(state: ForgeState) -> ForgeCounts {
    ForgeCounts {
        state: state as i32,
        ..ForgeCounts::default()
    }
}

/// 실행 결과에서 문제를 고른다. 문제가 없으면 없음.
fn problem(ran: &Ran) -> Option<ForgeState> {
    if !ran.started {
        return Some(ForgeState::Missing);
    }
    if ran.code == Some(0) {
        return None;
    }
    let text = String::from_utf8_lossy(&ran.stderr).to_lowercase();
    let signed_out = [
        "auth login",
        "not logged",
        "401",
        "unauthorized",
        "unauthenticated",
        "no token",
        "authentication",
    ]
    .iter()
    .any(|hint| text.contains(hint));
    Some(if signed_out {
        ForgeState::SignedOut
    } else {
        ForgeState::Failed
    })
}

fn numbers(output: &[u8]) -> Vec<u32> {
    String::from_utf8_lossy(output)
        .split_whitespace()
        .map(|word| word.trim_matches('"').parse::<u32>())
        .collect::<Result<_, _>>()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;

    /// 프로그램 이름과 첫 인자 몇 개로 답을 고르는 가짜
    struct Script(Mutex<Vec<(String, Ran)>>, Mutex<Vec<String>>);

    impl Runner for Script {
        fn run(&self, program: &Path, args: &[&str]) -> Ran {
            let call = format!("{} {}", program.display(), args.join(" "));
            self.1.lock().unwrap().push(call.clone());
            let mut script = self.0.lock().unwrap();
            let index = script
                .iter()
                .position(|(prefix, _)| call.starts_with(prefix.as_str()))
                .expect("정해 둔 답");
            script.remove(index).1
        }
    }

    fn ok(out: &str) -> Ran {
        Ran {
            code: Some(0),
            started: true,
            stdout: out.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn failed(err: &str) -> Ran {
        Ran {
            code: Some(1),
            started: true,
            stdout: Vec::new(),
            stderr: err.as_bytes().to_vec(),
        }
    }

    fn scripted(answers: Vec<(&str, Ran)>) -> (Forgery, Arc<Script>) {
        let script = Arc::new(Script(
            Mutex::new(
                answers
                    .into_iter()
                    .map(|(p, r)| (p.to_owned(), r))
                    .collect(),
            ),
            Mutex::default(),
        ));
        let forgery = Forgery::with(
            script.clone(),
            Some(PathBuf::from("/bin/gh")),
            Some(PathBuf::from("/bin/glab")),
        );
        (forgery, script)
    }

    #[test]
    fn counts_from_both() {
        let (forgery, script) = scripted(vec![
            ("/bin/gh api graphql", ok("\"3 1\"\n")),
            ("/bin/gh api notifications", ok("72\n")),
            (
                "/bin/glab api graphql",
                ok(
                    r#"{"data":{"currentUser":{"reviewRequestedMergeRequests":{"count":2},"authoredMergeRequests":{"count":4},"todos":{"count":7}}}}"#,
                ),
            ),
        ]);
        let forge = forgery.counts();
        let github = forge.github.unwrap();
        assert_eq!((github.reviews, github.mine, github.notices), (3, 1, 50));
        assert_eq!(github.state, ForgeState::Ok as i32);
        let gitlab = forge.gitlab.unwrap();
        assert_eq!((gitlab.reviews, gitlab.mine, gitlab.notices), (2, 4, 7));
        let calls = script.1.lock().unwrap();
        assert!(calls[0].contains("--hostname github.com"));
        assert!(calls[0].contains("archived:false"));
    }

    #[test]
    fn missing_signed_out_and_failed() {
        let (forgery, _) = scripted(vec![
            (
                "/bin/gh api graphql",
                failed("To get started with GitHub CLI, please run:  gh auth login"),
            ),
            (
                "/bin/glab api graphql",
                ok(r#"{"data":{"currentUser":null}}"#),
            ),
        ]);
        let forge = forgery.counts();
        assert_eq!(forge.github.unwrap().state, ForgeState::SignedOut as i32);
        assert_eq!(forge.gitlab.unwrap().state, ForgeState::SignedOut as i32);

        let (forgery, _) = scripted(vec![
            ("/bin/gh api graphql", failed("HTTP 502: bad gateway")),
            ("/bin/glab api graphql", ok("not json")),
        ]);
        let forge = forgery.counts();
        assert_eq!(forge.github.unwrap().state, ForgeState::Failed as i32);
        assert_eq!(forge.gitlab.unwrap().state, ForgeState::Failed as i32);

        let none = Forgery::with(
            Arc::new(Script(Mutex::default(), Mutex::default())),
            None,
            None,
        );
        let forge = none.counts();
        assert_eq!(forge.github.unwrap().state, ForgeState::Missing as i32);
        assert_eq!(forge.gitlab.unwrap().state, ForgeState::Missing as i32);
    }

    #[test]
    fn timeouts_are_failures() {
        let (forgery, _) = scripted(vec![
            (
                "/bin/gh api graphql",
                Ran {
                    code: None,
                    started: true,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
            ),
            ("/bin/glab api graphql", ok("{}")),
        ]);
        let forge = forgery.counts();
        assert_eq!(forge.github.unwrap().state, ForgeState::Failed as i32);
        assert_eq!(forge.gitlab.unwrap().state, ForgeState::Failed as i32);
    }

    #[test]
    #[ignore = "이 맥의 gh, glab 을 실제로 부른다. 숫자와 상태만 찍는다"]
    fn real() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let forge = Forgery::system(home).counts();
        for (name, counts) in [("github", forge.github), ("gitlab", forge.gitlab)] {
            let counts = counts.unwrap();
            eprintln!(
                "{name}: 상태 {:?}, 검토 {}, 내 요청 {}, 알림 {}",
                ForgeState::try_from(counts.state),
                counts.reviews,
                counts.mine,
                counts.notices
            );
        }
    }
}
