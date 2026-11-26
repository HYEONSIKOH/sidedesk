//! AI 코딩 도구(Claude Code, Codex)가 컴퓨터에 남기는 기록을 읽어 세션 단계를 알아낸다.
//! 기록은 도구의 내부 모양이라 바뀔 수 있다(의문점 9). 모르는 줄은 건너뛴다.
//! 대화 글자는 남기지 않고, 오류 문장에 경로나 기록 내용을 넣지 않는다.

mod claude;
mod codex;
mod tail;

use std::path::PathBuf;
use std::sync::Mutex;

pub use claude::Claude;
pub use codex::Codex;
use sidedesk_domain::{PortError, SessionFacts, SessionLog};

/// 제목과 폴더 이름에 남기는 글자 수
const TITLE_LEN: usize = 60;

/// 두 도구를 함께 읽는다. 한쪽이 실패해도 다른 쪽은 돌려준다.
pub struct Agents {
    claude: Mutex<Claude>,
    codex: Mutex<Codex>,
}

impl Agents {
    /// `home` 은 사용자 홈 폴더
    pub fn new(home: PathBuf) -> Self {
        Self {
            claude: Mutex::new(Claude::new(home.join(".claude"))),
            codex: Mutex::new(Codex::new(home.join(".codex"))),
        }
    }
}

impl SessionLog for Agents {
    fn poll(&self) -> Result<Vec<SessionFacts>, PortError> {
        let mut facts = lock(&self.claude).poll();
        facts.extend(lock(&self.codex).poll());
        Ok(facts)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|lock| lock.into_inner())
}

/// 줄 바꿈과 제어 문자를 지우고 60글자로 자른다.
fn clean(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(TITLE_LEN).collect()
}

/// 경로의 마지막 이름
fn folder_name(path: &str) -> String {
    clean(
        path.trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "이 맥의 실제 기록을 읽는다. 도구와 단계별 수만 찍는다"]
    fn real() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let agents = Agents::new(home);
        let start = std::time::Instant::now();
        let facts = agents.poll().unwrap();
        let first = start.elapsed();
        let start = std::time::Instant::now();
        agents.poll().unwrap();
        let again = start.elapsed();
        let mut counts = std::collections::BTreeMap::new();
        for fact in &facts {
            *counts
                .entry(format!(
                    "{:?} {:?} 떠 있음 {}",
                    fact.tool, fact.phase, fact.open
                ))
                .or_insert(0) += 1;
        }
        eprintln!("세션 {}개, 처음 {first:?}, 다시 {again:?}", facts.len());
        for (kind, count) in counts {
            eprintln!("  {kind}: {count}");
        }
    }

    #[test]
    fn titles_are_flattened_and_cut() {
        assert_eq!(clean("  둘째\n줄\t탭\u{7} "), "둘째 줄 탭");
        assert_eq!(clean(&"가".repeat(100)).chars().count(), TITLE_LEN);
        assert_eq!(folder_name("/Users/me/work/side-desk/"), "side-desk");
        assert_eq!(folder_name(r"C:\work\app"), "app");
    }
}
