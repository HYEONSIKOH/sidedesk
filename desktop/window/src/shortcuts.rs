//! 설정 창의 단축키 칸. 기기 칸(board.rs)과 따로 두고, 화면에는 `renderShortcuts(json)` 으로 그린다.
//! 목록에는 보일 이름만 담고, 전체 경로와 주소는 고치기 상자를 열 때 그 하나만 보낸다.

use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use serde::Deserialize;
use serde_json::{Value, json};
use sidedesk_domain::{
    Action, AppChoice, Deck, PATH_LEN, Shortcut, ShortcutHelp, SystemAction, URL_LEN,
};

type Listener = Box<dyn Fn(String) + Send>;

/// 화면이 보낸 것이 고르기 창 요청이면 폴더인지 여부
pub(crate) fn pick_request(body: &str) -> Option<bool> {
    match body.trim() {
        r#"{"shortcut-pick":true}"# => Some(false),
        r#"{"shortcut-pick-folder":true}"# => Some(true),
        _ => None,
    }
}

#[derive(Default)]
struct State {
    apps: Option<Vec<AppChoice>>,
    focus: Option<Vec<String>>,
    loading: bool,
    message: Option<(String, bool)>,
    picked: Option<String>,
    detail: Option<Value>,
}

pub struct ShortcutsPanel {
    deck: Arc<Deck>,
    help: Arc<dyn ShortcutHelp>,
    state: Mutex<State>,
    changed: Mutex<Option<Listener>>,
}

#[derive(Deserialize)]
struct Draft {
    id: Option<u32>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    color: String,
    action: Action,
}

#[derive(Deserialize)]
struct Move {
    id: u32,
    step: i32,
}

impl ShortcutsPanel {
    pub fn new(deck: Arc<Deck>, help: Arc<dyn ShortcutHelp>) -> Self {
        Self {
            deck,
            help,
            state: Mutex::default(),
            changed: Mutex::new(None),
        }
    }

    pub fn on_change(&self, send: impl Fn(String) + Send + 'static) {
        *self.changed.lock().unwrap_or_else(|l| l.into_inner()) = Some(Box::new(send));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|l| l.into_inner())
    }

    /// 이 칸의 명령이면 처리하고 true. 메인 스레드에서 부른다(파일 고르기 창).
    pub fn command(&'static self, body: &str) -> bool {
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(body) else {
            return false;
        };
        let Some((name, value)) = map.into_iter().next() else {
            return false;
        };
        if !name.starts_with("shortcut-") {
            return false;
        }
        let outcome: Result<Option<&str>, String> = match name.as_str() {
            "shortcut-open" => {
                self.load_choices();
                Ok(None)
            }
            "shortcut-save" => self.save(value).map(|added| {
                Some(if added {
                    "더했습니다"
                } else {
                    "고쳤습니다"
                })
            }),
            "shortcut-remove" => value
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or_else(|| "없는 단축키입니다".to_owned())
                .and_then(|id| self.deck.remove(id))
                .map(|()| Some("지웠습니다")),
            "shortcut-move" => serde_json::from_value::<Move>(value)
                .map_err(|_| "없는 단축키입니다".to_owned())
                .and_then(|m| self.deck.shift(m.id, m.step))
                .map(|()| None),
            "shortcut-detail" => {
                let detail = value.as_u64().and_then(|id| {
                    self.deck
                        .items()
                        .into_iter()
                        .find(|s| u64::from(s.id) == id)
                });
                self.lock().detail = detail.map(|s| json!(s));
                Ok(None)
            }
            // 창에서는 view 가 먼저 가로채 이벤트 루프에서 pick() 을 부른다. 이 길은 시험과 창 없는 쪽에서만 쓴다.
            "shortcut-pick" => {
                let picked = self.help.pick_path();
                self.lock().picked = picked;
                Ok(None)
            }
            "shortcut-pick-folder" => {
                let picked = self.help.pick_folder();
                self.lock().picked = picked;
                Ok(None)
            }
            "shortcut-key-settings" => {
                self.help.open_key_settings();
                Ok(None)
            }
            _ => return false,
        };
        let mut state = self.lock();
        state.message = match outcome {
            Ok(Some(text)) => Some((text.to_owned(), true)),
            Ok(None) => None,
            Err(text) => Some((text, false)),
        };
        drop(state);
        self.publish();
        true
    }

    /// 고르기 창을 띄우고 고른 것을 화면에 알린다. 이벤트 루프(UI 스레드)에서 부른다.
    pub fn pick(&self, folder: bool) {
        let picked = if folder {
            self.help.pick_folder()
        } else {
            self.help.pick_path()
        };
        self.lock().picked = picked;
        self.publish();
    }

    /// 앱 목록과 단축어 목록은 느릴 수 있어 따로 스레드에서 한 번
    fn load_choices(&'static self) {
        {
            let mut state = self.lock();
            if state.loading {
                return;
            }
            state.loading = true;
        }
        thread::spawn(move || {
            let apps = self.help.apps();
            let focus = self.help.focus_shortcuts();
            let mut state = self.lock();
            state.apps = Some(apps);
            state.focus = Some(focus);
            state.loading = false;
            drop(state);
            self.publish();
        });
    }

    /// 새것이면 true
    fn save(&self, value: Value) -> Result<bool, String> {
        let draft: Draft =
            serde_json::from_value(value).map_err(|_| "입력을 읽지 못했습니다".to_owned())?;
        let too_long = match &draft.action {
            Action::Open { path } => path.len() > PATH_LEN,
            Action::Url { url } => url.len() > URL_LEN,
            _ => false,
        };
        if too_long {
            return Err("너무 깁니다".to_owned());
        }
        let shortcut = Shortcut {
            id: draft.id.unwrap_or(0),
            name: draft.name,
            icon: draft.icon,
            color: draft.color,
            action: draft.action,
        };
        match draft.id {
            Some(_) => self.deck.update(shortcut).map(|()| false),
            None => self.deck.add(shortcut).map(|_| true),
        }
    }

    pub fn snapshot(&self) -> String {
        let items: Vec<Value> = self
            .deck
            .items()
            .iter()
            .map(|shortcut| row(shortcut, self.help.as_ref()))
            .collect();
        let needs_keys = self.deck.items().iter().any(|s| {
            matches!(
                s.action,
                Action::Keys { .. }
                    | Action::System {
                        action: SystemAction::Lock
                    }
            )
        });
        let mut state = self.lock();
        let apps = state.apps.as_ref().map(|apps| {
            apps.iter()
                .map(|a| json!({"name": a.name, "bundle": a.bundle, "icon": a.icon, "color": a.color}))
                .collect::<Vec<_>>()
        });
        let value = json!({
            "items": items,
            "notice": self.deck.notice(),
            "keysAllowed": self.deck.can_send_keys(),
            "needsKeys": needs_keys,
            "apps": apps,
            "focus": state.focus,
            "focusSupported": self.help.focus_supported(),
            "folderPick": self.help.separate_folder_pick(),
            "loading": state.loading,
            "message": state.message.as_ref().map(|(text, good)| json!({"text": text, "good": good})),
            // 한 번만 보내는 것
            "picked": state.picked.take(),
            "detail": state.detail.take(),
        });
        value.to_string()
    }

    /// 화면이 있을 때만 그린다(한 번만 보내는 값을 화면 없이 써 버리지 않게).
    fn publish(&self) {
        let changed = self.changed.lock().unwrap_or_else(|l| l.into_inner());
        if let Some(send) = changed.as_ref() {
            send(self.snapshot());
        }
    }
}

/// 목록 한 줄: 보일 것만(전체 경로, 주소, 키 이름은 넣지 않음)
fn row(shortcut: &Shortcut, help: &dyn ShortcutHelp) -> Value {
    let (kind, summary, problem) = match &shortcut.action {
        Action::App { label, bundle } => (
            "app",
            label.clone(),
            (!help.app_exists(bundle)).then_some("앱 없음"),
        ),
        Action::Open { path } => {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let missing = (!std::path::Path::new(path).exists()).then_some("파일 없음");
            ("open", name, missing)
        }
        Action::Url { url } => {
            let host = url
                .split_once("//")
                .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
                .unwrap_or("");
            ("url", host.to_owned(), None)
        }
        Action::Keys { combo } => {
            let mut text = String::new();
            for (on, mark) in [
                (combo.control, "⌃"),
                (combo.option, "⌥"),
                (combo.shift, "⇧"),
                (combo.command, "⌘"),
            ] {
                if on {
                    text.push_str(mark);
                }
            }
            text.push_str(
                combo
                    .code
                    .trim_start_matches("Key")
                    .trim_start_matches("Digit"),
            );
            ("keys", text, None)
        }
        Action::System { action } => (
            "system",
            match action {
                SystemAction::Mute => "소리 끄기/켜기",
                SystemAction::Lock => "화면 잠금",
                SystemAction::Focus { .. } => "방해 금지",
            }
            .to_owned(),
            None,
        ),
    };
    json!({
        "id": shortcut.id,
        "name": shortcut.name,
        "icon": shortcut.icon,
        "color": shortcut.color,
        "kind": kind,
        "summary": summary,
        "problem": problem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidedesk_domain::{Launcher, PortError, Saved, ShortcutStore};

    #[derive(Default)]
    struct Memory(Mutex<Option<Saved>>);

    impl ShortcutStore for Memory {
        fn load(&self) -> Result<(Saved, Option<String>), PortError> {
            Ok((
                self.0
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| Saved::new(9)),
                None,
            ))
        }
        fn save(&self, saved: &Saved) -> Result<(), PortError> {
            *self.0.lock().unwrap() = Some(saved.clone());
            Ok(())
        }
    }

    struct Help;

    impl Launcher for Help {
        fn run(&self, _: &Action) -> Result<(), PortError> {
            Ok(())
        }
        fn toggle(&self, _: &Action) -> Option<bool> {
            None
        }
        fn can_send_keys(&self) -> bool {
            false
        }
    }

    impl ShortcutHelp for Help {
        fn apps(&self) -> Vec<AppChoice> {
            vec![AppChoice {
                name: "편집기".into(),
                bundle: "dev.sample.editor".into(),
                icon: "code".into(),
                color: "purple".into(),
            }]
        }
        fn pick_path(&self) -> Option<String> {
            Some("/work/docs/plan.txt".into())
        }
        fn focus_shortcuts(&self) -> Vec<String> {
            vec!["집중 켜기".into()]
        }
        fn app_exists(&self, bundle: &str) -> bool {
            bundle != "gone.app"
        }
    }

    fn panel() -> &'static ShortcutsPanel {
        let deck = Deck::new(Box::new(Memory::default()), Arc::new(Help));
        Box::leak(Box::new(ShortcutsPanel::new(deck, Arc::new(Help))))
    }

    fn snapshot(panel: &ShortcutsPanel) -> Value {
        serde_json::from_str(&panel.snapshot()).unwrap()
    }

    #[test]
    fn add_edit_move_remove() {
        let panel = panel();
        let url = r#"{"shortcut-save":{"name":"문서","icon":"browser","color":"orange","action":{"kind":"url","url":"https://docs.example.com/a/b"}}}"#;
        assert!(panel.command(url));
        let keys = r#"{"shortcut-save":{"name":"찾기","icon":"bolt","color":"sky","action":{"kind":"keys","combo":{"code":"KeyF","command":true,"shift":true}}}}"#;
        assert!(panel.command(keys));
        let now = snapshot(panel);
        let items = now["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["summary"], "docs.example.com");
        assert_eq!(items[1]["summary"], "⇧⌘F");
        // 목록에는 전체 주소가 없다.
        assert!(!now.to_string().contains("/a/b"));
        assert_eq!(now["needsKeys"], true);
        assert_eq!(now["keysAllowed"], false);
        // 맨 아래를 아래로는 그대로
        assert!(panel.command(r#"{"shortcut-move":{"id":2,"step":1}}"#));
        assert!(panel.command(r#"{"shortcut-move":{"id":2,"step":-1}}"#));
        assert_eq!(snapshot(panel)["items"][0]["id"], 2);
        // 고치기 상자를 열면 그 하나만 전체를 받는다.
        assert!(panel.command(r#"{"shortcut-detail":1}"#));
        let detail = snapshot(panel);
        assert_eq!(
            detail["detail"]["action"]["url"],
            "https://docs.example.com/a/b"
        );
        assert!(snapshot(panel)["detail"].is_null());
        assert!(panel.command(
            r#"{"shortcut-save":{"id":1,"name":"문서2","icon":"browser","color":"orange","action":{"kind":"url","url":"https://docs.example.com"}}}"#
        ));
        assert!(panel.command(r#"{"shortcut-remove":1}"#));
        assert!(panel.command(r#"{"shortcut-remove":1}"#));
        assert_eq!(snapshot(panel)["message"]["good"], false);
        assert!(panel.command(r#"{"shortcut-move":{"id":1,"step":1}}"#));
        assert_eq!(snapshot(panel)["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn odd_names_round_trip_and_unknown_kinds_are_refused() {
        let panel = panel();
        let odd = json!({"shortcut-save": {"name": "따옴\"표\n<b>", "icon": "star", "color": "gray",
            "action": {"kind": "url", "url": "https://example.com"}}});
        assert!(panel.command(&odd.to_string()));
        let now = snapshot(panel);
        assert_eq!(now["items"][0]["name"], "따옴\"표<b>");
        assert!(
            panel.command(
                r#"{"shortcut-save":{"name":"x","action":{"kind":"shell","command":"rm"}}}"#
            )
        );
        assert_eq!(snapshot(panel)["message"]["good"], false);
        // 넘치는 번호는 자르지 않고 거절한다.
        assert!(panel.command(r#"{"shortcut-remove":4294967297}"#));
        assert_eq!(snapshot(panel)["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn missing_apps_are_marked() {
        let panel = panel();
        assert!(panel.command(
            r#"{"shortcut-save":{"name":"없는 앱","action":{"kind":"app","bundle":"gone.app","label":"사라짐"}}}"#
        ));
        assert_eq!(snapshot(panel)["items"][0]["problem"], "앱 없음");
    }

    #[test]
    fn refusals_show_a_reason() {
        let panel = panel();
        assert!(panel.command(
            r#"{"shortcut-save":{"name":"x","action":{"kind":"url","url":"file:///etc/hosts"}}}"#
        ));
        let now = snapshot(panel);
        assert_eq!(now["message"]["good"], false);
        assert!(now["items"].as_array().unwrap().is_empty());
        // 다른 명령은 이 칸이 받지 않는다.
        assert!(!panel.command(r#"{"pair":"a"}"#));
        assert!(!panel.command("nonsense"));
    }

    #[test]
    fn picking_and_choices() {
        let panel = panel();
        assert!(panel.command(r#"{"shortcut-pick":true}"#));
        assert_eq!(snapshot(panel)["picked"], "/work/docs/plan.txt");
        assert!(snapshot(panel)["picked"].is_null());
        assert!(panel.command(r#"{"shortcut-open":true}"#));
        // 느린 CI 서버를 생각해 최대 20초
        for _ in 0..4000 {
            if snapshot(panel)["apps"].is_array() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(5));
        }
        let now = snapshot(panel);
        assert_eq!(now["apps"][0]["bundle"], "dev.sample.editor");
        assert_eq!(now["focus"][0], "집중 켜기");
        // 기본값: 방해 금지를 고를 수 있고 폴더 고르기는 따로 없다(맥).
        assert_eq!(now["focusSupported"], true);
        assert_eq!(now["folderPick"], false);
        // 폴더 고르기를 지원하지 않으면 고른 것이 없다.
        assert!(panel.command(r#"{"shortcut-pick-folder":true}"#));
        assert!(snapshot(panel)["picked"].is_null());
    }
}
