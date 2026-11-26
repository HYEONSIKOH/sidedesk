//! AI 세션 장부. 기록에서 읽은 단계로 상태를 정하고, 바뀐 때 알림을 만든다.
//! 시각은 벽시계로 받는다.

use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, RandomState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sidedesk_core::{AiAlert, AiAlertKind, AiSession, AiSessions, AiState, Forge};

use crate::port::{Phase, SessionFacts};

#[derive(Clone, Copy, Debug)]
pub struct AiCadence {
    /// 기록을 다시 보는 간격
    pub poll: Duration,
    /// 답을 마친 뒤 이만큼 새 기록이 없으면 끝남(설계 9절)
    pub done_after: Duration,
    /// 닫힌 세션을 목록에 남기는 시간
    pub keep_closed: Duration,
    pub forge_every: Duration,
    pub max_items: usize,
    /// 폰이 못 따라갈 때 쌓아 두는 알림 수
    pub max_alerts: usize,
}

impl Default for AiCadence {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(1),
            done_after: Duration::from_secs(10),
            keep_closed: Duration::from_secs(6 * 3600),
            forge_every: Duration::from_secs(180),
            max_items: 12,
            max_alerts: 64,
        }
    }
}

struct Known {
    id: String,
    facts: SessionFacts,
    state: AiState,
    since: SystemTime,
}

#[derive(Default)]
struct Want {
    sessions: bool,
    forge: bool,
}

pub(crate) struct AiBook {
    salt: RandomState,
    known: HashMap<String, Known>,
    wants: HashMap<u64, Want>,
    lists: HashMap<u64, AiSessions>,
    alerts: HashMap<u64, VecDeque<AiAlert>>,
    last_list: Option<AiSessions>,
    next_alert: u64,
    /// 읽기를 (다시) 시작한 첫 poll 은 기준만 잡고 알리지 않는다.
    baseline: bool,
    forge_latest: Option<(SystemTime, Forge)>,
    forge_tried: Option<SystemTime>,
    forge_out: HashMap<u64, Forge>,
}

impl Default for AiBook {
    fn default() -> Self {
        Self {
            salt: RandomState::new(),
            known: HashMap::new(),
            wants: HashMap::new(),
            lists: HashMap::new(),
            alerts: HashMap::new(),
            last_list: None,
            next_alert: 1,
            baseline: true,
            forge_latest: None,
            forge_tried: None,
            forge_out: HashMap::new(),
        }
    }
}

impl AiBook {
    pub fn set_wants(&mut self, id: u64, sessions: bool, forge: bool) {
        let want = self.wants.entry(id).or_default();
        let first_sessions = sessions && !want.sessions;
        let first_forge = forge && !want.forge;
        *want = Want { sessions, forge };
        if !sessions {
            self.lists.remove(&id);
            self.alerts.remove(&id);
        } else if first_sessions && let Some(list) = &self.last_list {
            self.lists.insert(id, list.clone());
        }
        if !forge {
            self.forge_out.remove(&id);
        } else if first_forge && let Some((_, forge)) = &self.forge_latest {
            self.forge_out.insert(id, *forge);
        }
        if !self.watching() {
            self.baseline = true;
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.wants.remove(&id);
        self.lists.remove(&id);
        self.alerts.remove(&id);
        self.forge_out.remove(&id);
        if !self.watching() {
            self.baseline = true;
        }
    }

    /// 세션을 보는 구독자가 있는지
    pub fn watching(&self) -> bool {
        self.wants.values().any(|want| want.sessions)
    }

    pub fn take(&mut self, id: u64) -> (Option<AiSessions>, Vec<AiAlert>, Option<Forge>) {
        let list = self.lists.remove(&id);
        let alerts = self
            .alerts
            .get_mut(&id)
            .map(|queue| queue.drain(..).collect())
            .unwrap_or_default();
        (list, alerts, self.forge_out.remove(&id))
    }

    /// 기록에서 읽은 사실을 반영한다.
    pub fn apply(&mut self, facts: Vec<SessionFacts>, now: SystemTime, cadence: &AiCadence) {
        let baseline = std::mem::take(&mut self.baseline);
        let mut fresh = HashMap::new();
        let mut alerts = Vec::new();
        for fact in facts {
            let (state, since, alert) = judge(&fact, now, cadence);
            let id = match self.known.remove(&fact.key) {
                Some(old) => {
                    let changed = old.state != state;
                    if changed
                        && !baseline
                        && old.state == AiState::Working
                        && let Some(kind) = alert
                    {
                        alerts.push((old.id.clone(), fact.title.clone(), kind, since));
                    }
                    let since = if changed { since } else { old.since };
                    fresh.insert(
                        fact.key.clone(),
                        Known {
                            id: old.id,
                            facts: fact,
                            state,
                            since,
                        },
                    );
                    continue;
                }
                None => self.short_id(&fact.key),
            };
            fresh.insert(
                fact.key.clone(),
                Known {
                    id,
                    facts: fact,
                    state,
                    since,
                },
            );
        }
        // 기록이 지워진 세션은 기억에서도 뺀다.
        self.known = fresh;
        for (session_id, title, kind, at) in alerts {
            let alert = AiAlert {
                id: self.next_alert,
                session_id,
                title,
                kind: kind as i32,
                at: millis(at),
            };
            self.next_alert += 1;
            for (id, want) in &self.wants {
                if want.sessions {
                    let queue = self.alerts.entry(*id).or_default();
                    if queue.len() >= cadence.max_alerts {
                        queue.pop_front();
                    }
                    queue.push_back(alert.clone());
                }
            }
        }
        let list = self.list(now, cadence);
        if self.last_list.as_ref() != Some(&list) {
            for (id, want) in &self.wants {
                if want.sessions {
                    self.lists.insert(*id, list.clone());
                }
            }
            self.last_list = Some(list);
        }
    }

    fn list(&self, now: SystemTime, cadence: &AiCadence) -> AiSessions {
        let mut items: Vec<&Known> = self
            .known
            .values()
            .filter(|known| {
                known.facts.open || elapsed(known.facts.last_activity, now) < cadence.keep_closed
            })
            .collect();
        let rank = |state: AiState| match state {
            AiState::Waiting | AiState::Working => 0,
            AiState::Done => 1,
            AiState::Unknown => 2,
        };
        items.sort_by(|a, b| {
            rank(a.state)
                .cmp(&rank(b.state))
                .then(b.since.cmp(&a.since))
                .then(a.id.cmp(&b.id))
        });
        AiSessions {
            items: items
                .into_iter()
                .take(cadence.max_items)
                .map(|known| AiSession {
                    id: known.id.clone(),
                    tool: known.facts.tool as i32,
                    title: known.facts.title.clone(),
                    project: known.facts.project.clone(),
                    state: known.state as i32,
                    since: millis(known.since),
                    open: known.facts.open,
                })
                .collect(),
        }
    }

    fn short_id(&self, key: &str) -> String {
        format!("{:012x}", self.salt.hash_one(key) & 0xffff_ffff_ffff)
    }

    /// 지금 GitHub/GitLab 수를 받아야 하는지
    pub fn forge_due(&self, now: SystemTime, cadence: &AiCadence) -> bool {
        self.wants.values().any(|want| want.forge)
            && self
                .forge_tried
                .is_none_or(|tried| elapsed(tried, now) >= cadence.forge_every || tried > now)
    }

    pub fn forge_tried(&mut self, now: SystemTime) {
        self.forge_tried = Some(now);
    }

    pub fn forge_wait(&self, now: SystemTime, cadence: &AiCadence) -> Option<Duration> {
        if !self.wants.values().any(|want| want.forge) {
            return None;
        }
        Some(
            self.forge_tried
                .map_or(Duration::ZERO, |tried| {
                    cadence.forge_every.saturating_sub(elapsed(tried, now))
                })
                .min(cadence.poll * 60),
        )
    }

    pub fn forge_publish(&mut self, forge: Forge, now: SystemTime) {
        for (id, want) in &self.wants {
            if want.forge {
                self.forge_out.insert(*id, forge);
            }
        }
        self.forge_latest = Some((now, forge));
    }
}

/// 단계 → (상태, 그 상태가 된 때, 알림 종류)
fn judge(
    fact: &SessionFacts,
    now: SystemTime,
    cadence: &AiCadence,
) -> (AiState, SystemTime, Option<AiAlertKind>) {
    let last = fact.last_activity;
    let phase = match fact.phase {
        // 작업 중에 닫히면 사람이 멈춘 것과 같이 본다.
        Phase::Working | Phase::Waiting if !fact.open => Phase::Stopped,
        phase => phase,
    };
    match phase {
        Phase::Working => (AiState::Working, last, None),
        Phase::Waiting => (AiState::Waiting, last, Some(AiAlertKind::Waiting)),
        Phase::TurnEnded => {
            let done_at = last + cadence.done_after;
            if now >= done_at {
                (AiState::Done, done_at, Some(AiAlertKind::Done))
            } else {
                (AiState::Working, last, None)
            }
        }
        Phase::Stopped => (AiState::Done, last, None),
        Phase::Unknown => (AiState::Unknown, last, None),
    }
}

fn elapsed(since: SystemTime, now: SystemTime) -> Duration {
    now.duration_since(since).unwrap_or(Duration::ZERO)
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidedesk_core::{AiTool, ForgeCounts, ForgeState};

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
    }

    fn fact(key: &str, phase: Phase, last: u64) -> SessionFacts {
        SessionFacts {
            key: key.into(),
            tool: AiTool::ClaudeCode,
            title: format!("{key} 제목"),
            project: "폴더".into(),
            open: true,
            phase,
            last_activity: at(last),
        }
    }

    fn book() -> (AiBook, AiCadence) {
        let c = AiCadence::default();
        let mut book = AiBook::default();
        book.set_wants(1, true, false);
        (book, c)
    }

    fn states(book: &mut AiBook) -> Vec<(String, i32)> {
        let (list, _, _) = book.take(1);
        list.map(|l| l.items.into_iter().map(|s| (s.title, s.state)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn done_after_ten_quiet_seconds_and_one_alert() {
        let (mut book, c) = book();
        book.apply(vec![fact("a", Phase::Working, 0)], at(0), &c);
        book.take(1);
        // 9초 뒤에는 아직 작업 중. 목록도 그대로라 다시 보내지 않는다.
        book.apply(vec![fact("a", Phase::TurnEnded, 100)], at(109), &c);
        assert_eq!(book.take(1), (None, vec![], None));
        book.apply(vec![fact("a", Phase::TurnEnded, 100)], at(111), &c);
        let (list, alerts, _) = book.take(1);
        let item = &list.unwrap().items[0];
        assert_eq!(item.state, AiState::Done as i32);
        assert_eq!(item.since, millis(at(110)));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AiAlertKind::Done as i32);
        assert_eq!(alerts[0].at, millis(at(110)));
        assert_eq!(alerts[0].session_id, item.id);
        // 그대로면 목록도 알림도 다시 안 보낸다.
        book.apply(vec![fact("a", Phase::TurnEnded, 100)], at(130), &c);
        assert_eq!(book.take(1), (None, vec![], None));
    }

    #[test]
    fn working_again_within_ten_seconds_does_not_alert() {
        let (mut book, c) = book();
        book.apply(vec![fact("a", Phase::Working, 0)], at(0), &c);
        book.apply(vec![fact("a", Phase::TurnEnded, 10)], at(15), &c);
        book.apply(vec![fact("a", Phase::Working, 18)], at(18), &c);
        book.apply(vec![fact("a", Phase::Working, 18)], at(40), &c);
        let (_, alerts, _) = book.take(1);
        assert!(alerts.is_empty());
    }

    #[test]
    fn waiting_alerts_and_stopping_does_not() {
        let (mut book, c) = book();
        book.apply(
            vec![fact("a", Phase::Working, 0), fact("b", Phase::Working, 0)],
            at(0),
            &c,
        );
        let mut closed = fact("b", Phase::Working, 5);
        closed.open = false;
        book.apply(vec![fact("a", Phase::Waiting, 5), closed], at(6), &c);
        let (_, alerts, _) = book.take(1);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AiAlertKind::Waiting as i32);
        assert_eq!(alerts[0].title, "a 제목");
        // Esc 로 멈춤
        book.apply(vec![fact("a", Phase::Working, 7)], at(7), &c);
        book.apply(vec![fact("a", Phase::Stopped, 8)], at(30), &c);
        let (list, alerts, _) = book.take(1);
        assert!(alerts.is_empty());
        assert!(
            list.unwrap()
                .items
                .iter()
                .all(|s| s.state == AiState::Done as i32)
        );
    }

    #[test]
    fn first_sight_and_resumed_watching_do_not_alert() {
        let c = AiCadence::default();
        let mut book = AiBook::default();
        book.set_wants(1, true, false);
        // 켜자마자 이미 끝난 세션
        book.apply(vec![fact("a", Phase::TurnEnded, 0)], at(100), &c);
        assert!(book.take(1).1.is_empty());
        book.apply(vec![fact("b", Phase::Working, 100)], at(101), &c);
        // 구독이 끊긴 사이 b 가 끝남
        book.remove(1);
        book.set_wants(2, true, false);
        book.apply(vec![fact("b", Phase::TurnEnded, 110)], at(200), &c);
        let (list, alerts, _) = book.take(2);
        assert!(alerts.is_empty());
        assert!(list.is_some());
        // 그 뒤 바뀜은 알린다.
        book.apply(vec![fact("b", Phase::Working, 210)], at(210), &c);
        book.apply(vec![fact("b", Phase::Waiting, 211)], at(211), &c);
        assert_eq!(book.take(2).1.len(), 1);
    }

    #[test]
    fn order_cap_and_old_closed_sessions() {
        let (mut book, c) = book();
        let mut facts: Vec<_> = (0..15)
            .map(|n| fact(&format!("d{n:02}"), Phase::TurnEnded, n))
            .collect();
        facts.push(fact("w", Phase::Working, 1));
        let mut old = fact("old", Phase::TurnEnded, 0);
        old.open = false;
        facts.push(old);
        book.apply(facts, at(7 * 3600), &c);
        let list = states(&mut book);
        assert_eq!(list.len(), 12);
        assert_eq!(list[0].0, "w 제목");
        assert_eq!(list[1].0, "d14 제목");
        assert!(!list.iter().any(|(title, _)| title == "old 제목"));
    }

    #[test]
    fn deleted_records_are_forgotten() {
        let (mut book, c) = book();
        book.apply(vec![fact("a", Phase::Working, 0)], at(0), &c);
        book.apply(vec![], at(1), &c);
        assert!(states(&mut book).is_empty());
        assert!(book.known.is_empty());
    }

    #[test]
    fn two_phones_each_get_the_alert_and_others_get_nothing() {
        let c = AiCadence::default();
        let mut book = AiBook::default();
        book.set_wants(1, true, true);
        book.set_wants(2, true, false);
        book.set_wants(3, false, false);
        book.apply(vec![fact("a", Phase::Working, 0)], at(0), &c);
        book.apply(vec![fact("a", Phase::Waiting, 1)], at(1), &c);
        assert_eq!(book.take(1).1.len(), 1);
        assert_eq!(book.take(2).1.len(), 1);
        assert_eq!(book.take(3), (None, vec![], None));
    }

    #[test]
    fn alert_queue_keeps_up_to_the_cap() {
        let c = AiCadence {
            max_alerts: 3,
            ..AiCadence::default()
        };
        let mut book = AiBook::default();
        book.set_wants(1, true, false);
        book.apply(vec![fact("a", Phase::Working, 0)], at(0), &c);
        for n in 0..5 {
            book.apply(
                vec![fact("a", Phase::Waiting, 2 * n + 1)],
                at(2 * n + 1),
                &c,
            );
            book.apply(
                vec![fact("a", Phase::Working, 2 * n + 2)],
                at(2 * n + 2),
                &c,
            );
        }
        let ids: Vec<u64> = book.take(1).1.iter().map(|a| a.id).collect();
        assert_eq!(ids, [3, 4, 5]);
    }

    #[test]
    fn forge_only_for_main_and_every_three_minutes() {
        let c = AiCadence::default();
        let mut book = AiBook::default();
        book.set_wants(1, true, false);
        assert!(!book.forge_due(at(0), &c));
        book.set_wants(1, true, true);
        assert!(book.forge_due(at(0), &c));
        book.forge_tried(at(0));
        let counts = Forge {
            github: Some(ForgeCounts {
                state: ForgeState::Ok as i32,
                reviews: 1,
                mine: 2,
                notices: 3,
            }),
            gitlab: None,
        };
        book.forge_publish(counts, at(1));
        assert_eq!(book.take(1).2, Some(counts));
        assert!(!book.forge_due(at(179), &c));
        assert!(book.forge_due(at(180), &c));
        // 새로 큰 화면을 보면 받아 둔 값을 바로 준다.
        book.set_wants(2, true, true);
        assert_eq!(book.take(2).2, Some(counts));
    }
}
