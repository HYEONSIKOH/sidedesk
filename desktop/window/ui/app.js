'use strict';

const $ = (id) => document.getElementById(id);
const send = (message) => window.ipc.postMessage(JSON.stringify(message));
document.querySelector(".top").addEventListener("mousedown", (event) => {
  if (event.button === 0) window.ipc.postMessage("drag");
});
let state = null;
// 짝 끊기를 한 번 누른 키. 화면을 다시 그려도 확인 상태를 이어 간다.
let confirming = null;
let confirmTimer = 0;

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === 'class') node.className = value;
    else if (key === 'on') node.addEventListener('click', value);
    else node.setAttribute(key, value);
  }
  for (const child of children) if (child != null) node.append(child);
  return node;
}

const STATUS = {
  connected: ['ok', '연결됨'],
  lost: ['bad', '끊김'],
  refused: ['bad', '거절됨'],
  idle: ['idle', 'USB 연결 안 됨'],
  blocked: ['bad', '연결 불가'],
  waiting: ['idle', '연결 대기'],
  pairing: ['idle', '등록 중'],
};

function pill(status, reason) {
  const [tone, label] = STATUS[status] || STATUS.idle;
  return el('span', {}, el('span', { class: `pill ${tone}` }, label), reason ? el('span', { class: 'reason' }, reason) : null);
}

function show(page) {
  document.querySelectorAll('.menu-item').forEach((b) => b.classList.toggle('on', b.dataset.page === page));
  document.querySelectorAll('.page').forEach((p) => p.classList.toggle('on', p.id === page));
}

function forget(phone) {
  clearTimeout(confirmTimer);
  if (confirming !== phone.key) {
    confirming = phone.key;
    confirmTimer = setTimeout(() => { confirming = null; render(state); }, 4000);
  } else {
    confirming = null;
    send({ forget: phone.key });
  }
  render(state);
}

function render(next) {
  state = next;
  const busy = Boolean(state.pairing);

  const registered = state.paired.map((phone) => el('tr', {},
    el('td', {}, phone.name || '이름 없음'),
    el('td', { class: 'mono' }, phone.serial || '-'),
    el('td', {}, phone.link || '-'),
    el('td', {}, pill(phone.status, phone.reason)),
    el('td', { class: 'end' }, el('button', { class: 'button danger', on: () => forget(phone) },
      confirming === phone.key ? '한 번 더 누르면 해제' : '등록 해제')),
  ));
  const fresh = state.attached.filter((phone) => !phone.paired).map((phone) => {
    const pair = el('button', { class: 'button primary', on: () => send({ pair: phone.id }) }, '등록');
    pair.disabled = busy || phone.status === 'blocked';
    return el('tr', {},
      el('td', {}, phone.name),
      el('td', { class: 'mono' }, phone.id),
      el('td', {}, 'USB'),
      el('td', {}, pill(phone.status, phone.reason)),
      el('td', { class: 'end' }, pair),
    );
  });
  $('device-rows').replaceChildren(...registered, ...fresh);
  $('device-empty').hidden = registered.length + fresh.length > 0;

  $('problems').replaceChildren(...(state.problems || []).map((text) => el('p', { class: 'notice' }, text)));

  const notice = $('notice');
  notice.hidden = !state.notice;
  notice.textContent = state.notice ? state.notice.text : '';
  notice.classList.toggle('calm', Boolean(state.notice && state.notice.good));

  renderWeather(state.weather);

  $('computer-name').textContent = state.computer.name;
  $('version').textContent = state.computer.version;
  $('fingerprint').textContent = state.computer.fingerprint;

  $('pairing').hidden = !busy;
  if (busy) {
    $('pairing-phone').textContent = `${state.pairing.name}을(를) 등록합니다.`;
    const { code, waiting } = state.pairing;
    $('pairing-code').textContent = code ? `${code.slice(0, 3)} ${code.slice(3)}` : '··· ···';
    $('pairing-question').textContent = !code
      ? '기기에서 SideDesk 앱을 열어 두세요. 번호를 기다리는 중입니다.'
      : waiting ? '기기에서 등록을 누르길 기다리는 중입니다.' : '기기에 표시된 번호와 같으면 등록을 누르세요.';
    $('pairing-yes').disabled = !code || waiting;
    $('pairing-no').disabled = !code || waiting;
  }
}

function renderWeather(weather) {
  const on = Boolean(weather && weather.supported);
  $('weather-choice').disabled = !on;
  $('weather-hint').hidden = !on;
  $('weather-off').hidden = on;
  if (on) $('weather-choice').value = weather.choice;
  const kma = on && weather.choice === 'kma';
  $('kma-empty').hidden = !kma || weather.has_key;
  $('kma-saved').hidden = !kma || !weather.has_key;
  if (!kma) { $('kma-result').hidden = true; return; }
  $('kma-hint').value = weather.has_key ? `${weather.key_hint || ''}${'*'.repeat(12)}` : '';
  $('kma-put').disabled = weather.checking;
  $('kma-put').textContent = weather.checking ? '확인하는 중' : '넣기';
  const result = $('kma-result');
  result.hidden = !weather.result;
  result.textContent = weather.result ? weather.result.text : '';
  result.classList.toggle('bad', Boolean(weather.result && !weather.result.good));
}

document.querySelectorAll('.menu-item').forEach((b) => b.addEventListener('click', () => show(b.dataset.page)));
$('weather-choice').addEventListener('change', (event) => send({ weather: event.target.value }));
// 키는 보낸 뒤 칸을 비우고 어디에도 남기지 않는다.
$('kma-put').addEventListener('click', () => {
  const input = $('kma-key');
  const value = input.value.trim();
  input.value = '';
  if (value) send({ 'kma-key': value });
});
$('kma-clear').addEventListener('click', () => send({ 'kma-key-clear': '' }));
$('pairing-yes').addEventListener('click', () => send({ answer: true }));
$('pairing-no').addEventListener('click', () => send({ answer: false }));
const systemDark = window.matchMedia('(prefers-color-scheme: dark)');
function showMode() {
  const root = document.documentElement;
  const current = root.dataset.mode || 'auto';
  root.dataset.theme = current === 'auto' ? (systemDark.matches ? 'dark' : 'light') : current;
  document.querySelectorAll('.segment [data-mode]').forEach((b) => {
    const on = b.dataset.mode === current;
    b.classList.toggle('on', on);
    b.setAttribute('aria-checked', String(on));
  });
}
document.querySelectorAll('.segment [data-mode]').forEach((b) =>
  b.addEventListener('click', () => window.ipc.postMessage(`mode:${b.dataset.mode}`)));
systemDark.addEventListener('change', showMode);
showMode();
show('phones');
send({ ready: true });

// 단축키 칸. 화면에 넣는 글자는 모두 textContent 로만.
const ICON_GLYPH = {
  terminal: '>_', folder: '▤', code: '</>', browser: '◎', calculator: '±', settings: '⚙',
  activity: '∿', music: '♪', volume: '◁)', dnd: '☾', lock: '▣', file: '▯', star: '★', bolt: 'ϟ',
};
const COLOR_VALUE = {
  slate: '#4F7090', green: '#6C8A6E', purple: '#8A7AA8', orange: '#A8805A', teal: '#5E8A94',
  gray: '#747C88', olive: '#7A8F5E', rose: '#A86A7C', sand: '#E0B95C', sky: '#7FA2D9',
};
let shortcutState = null;
let shortcutConfirm = null;
const box = { saving: false, id: null, kind: 'app', icon: 'star', color: 'gray', combo: null, path: '', bundle: '', label: '' };

function chip(icon, color) {
  const node = el('span', { class: 'chip' }, ICON_GLYPH[icon] || '★');
  node.style.background = COLOR_VALUE[color] || COLOR_VALUE.gray;
  return node;
}

function comboText(combo) {
  if (!combo) return '';
  const key = combo.code.replace(/^Key|^Digit/, '');
  if (document.documentElement.classList.contains('mac')) {
    const marks = (combo.control ? '⌃' : '') + (combo.option ? '⌥' : '') + (combo.shift ? '⇧' : '') + (combo.command ? '⌘' : '');
    return marks + key;
  }
  // 윈도우는 글자로(command 는 Win 키)
  const names = [combo.control && 'Ctrl', combo.option && 'Alt', combo.shift && 'Shift', combo.command && 'Win'].filter(Boolean);
  return [...names, key].join('+');
}

function renderShortcuts(next) {
  shortcutState = next;
  // 윈도우: 방해 금지는 고를 수 없고, 폴더는 따로 고른다.
  const focusOption = $('sb-system').querySelector('option[value="focus"]');
  focusOption.hidden = focusOption.disabled = next.focusSupported === false;
  $('sb-pick-folder').hidden = !next.folderPick;
  const rows = next.items.map((item, index) => {
    const remove = el('button', { class: 'button danger', on: () => {
      if (shortcutConfirm === item.id) { shortcutConfirm = null; send({ 'shortcut-remove': item.id }); }
      else { shortcutConfirm = item.id; renderShortcuts(shortcutState); }
    } }, shortcutConfirm === item.id ? '한 번 더 누르면 지움' : '지우기');
    const up = el('button', { class: 'button outline', on: () => send({ 'shortcut-move': { id: item.id, step: -1 } }) }, '위로');
    const down = el('button', { class: 'button outline', on: () => send({ 'shortcut-move': { id: item.id, step: 1 } }) }, '아래로');
    up.disabled = index === 0;
    down.disabled = index === next.items.length - 1;
    const edit = el('button', { class: 'button outline', on: () => send({ 'shortcut-detail': item.id }) }, '고치기');
    const summary = el('td', {}, item.summary || '-');
    if (item.problem) summary.append(el('span', { class: 'problem' }, item.problem));
    return el('tr', {},
      el('td', {}, chip(item.icon, item.color)),
      el('td', {}, item.name || '이름 없음'),
      summary,
      el('td', { class: 'end' }, up, down, edit, remove));
  });
  $('shortcut-rows').replaceChildren(...rows);
  $('shortcut-empty').hidden = rows.length > 0;
  $('shortcut-notice').hidden = !next.notice;
  $('shortcut-notice').textContent = next.notice || '';
  $('shortcut-keys').hidden = !next.needsKeys || next.keysAllowed;
  const message = $('shortcut-message');
  message.hidden = !next.message;
  message.textContent = next.message ? next.message.text : '';
  message.classList.toggle('calm', Boolean(next.message && next.message.good));
  // 저장을 누른 뒤 온 답일 때만 상자를 닫거나 까닭을 보인다.
  if (box.saving && next.message) {
    box.saving = false;
    if (next.message.good) closeBox(); else showError(next.message.text);
  }
  fillChoices();
  if (next.picked) {
    box.path = next.picked;
    $('sb-path').textContent = next.picked;
    $('sb-path-name').textContent = next.picked.split('/').pop();
  }
  if (next.detail) openBox(next.detail);
}

function fillChoices() {
  const state = shortcutState || {};
  $('sb-app-loading').hidden = Boolean(state.apps);
  const find = $('sb-app-find').value.trim().toLowerCase();
  const apps = (state.apps || []).filter((app) => !find || app.name.toLowerCase().includes(find));
  const app = $('sb-app');
  app.replaceChildren(...apps.map((a) => { const o = el('option', { value: a.bundle }, a.name); o.selected = a.bundle === box.bundle; return o; }));
  for (const id of ['sb-focus-on', 'sb-focus-off']) {
    const keep = $(id).value;
    $(id).replaceChildren(...(state.focus || []).map((name) => el('option', { value: name }, name)));
    if (keep) $(id).value = keep;
  }
}

function showKind() {
  box.kind = $('sb-kind').value;
  document.querySelectorAll('#shortcut-box .kind').forEach((k) => { k.hidden = k.dataset.kind !== box.kind; });
  $('sb-focus').hidden = !(box.kind === 'system' && $('sb-system').value === 'focus');
}

function swatches() {
  $('sb-icons').replaceChildren(...Object.keys(ICON_GLYPH).map((icon) => {
    const b = el('button', { class: 'swatch' + (box.icon === icon ? ' on' : ''), title: icon, on: () => { box.icon = icon; swatches(); } }, ICON_GLYPH[icon]);
    b.style.background = COLOR_VALUE[box.color];
    return b;
  }));
  $('sb-colors').replaceChildren(...Object.keys(COLOR_VALUE).map((color) => {
    const b = el('button', { class: 'swatch' + (box.color === color ? ' on' : ''), title: color, on: () => { box.color = color; swatches(); } });
    b.style.background = COLOR_VALUE[color];
    return b;
  }));
}

function showError(text) {
  $('sb-error').hidden = !text;
  $('sb-error').textContent = text || '';
}

function openBox(detail) {
  Object.assign(box, { saving: false, id: null, kind: 'app', icon: 'star', color: 'gray', combo: null, path: '', bundle: '', label: '' });
  $('sb-name').value = '';
  $('sb-url').value = '';
  $('sb-keys').value = '';
  $('sb-path').textContent = '';
  $('sb-path-name').textContent = '';
  $('sb-system').value = 'mute';
  if (detail) {
    const a = detail.action;
    Object.assign(box, { id: detail.id, kind: a.kind, icon: detail.icon, color: detail.color });
    $('sb-name').value = detail.name;
    if (a.kind === 'app') { box.bundle = a.bundle; box.label = a.label; }
    if (a.kind === 'open') { box.path = a.path; $('sb-path').textContent = a.path; $('sb-path-name').textContent = a.path.split('/').pop(); }
    if (a.kind === 'url') $('sb-url').value = a.url;
    if (a.kind === 'keys') { box.combo = a.combo; $('sb-keys').value = comboText(a.combo); }
    if (a.kind === 'system') {
      $('sb-system').value = a.action.kind;
      if (a.action.kind === 'focus') { $('sb-focus-on').value = a.action.on; $('sb-focus-off').value = a.action.off; }
    }
  }
  $('sb-kind').value = box.kind;
  showError('');
  showKind();
  swatches();
  fillChoices();
  $('shortcut-box').hidden = false;
}

function closeBox() {
  $('shortcut-box').hidden = true;
}

function draftAction() {
  switch (box.kind) {
    case 'app': {
      const option = $('sb-app').selectedOptions[0];
      return option ? { kind: 'app', bundle: option.value, label: option.textContent } : (box.bundle ? { kind: 'app', bundle: box.bundle, label: box.label } : null);
    }
    case 'open': return box.path ? { kind: 'open', path: box.path } : null;
    case 'url': return { kind: 'url', url: $('sb-url').value.trim() };
    case 'keys': return box.combo ? { kind: 'keys', combo: box.combo } : null;
    case 'system': {
      const kind = $('sb-system').value;
      return kind === 'focus' ? { kind: 'system', action: { kind, on: $('sb-focus-on').value, off: $('sb-focus-off').value } } : { kind: 'system', action: { kind } };
    }
  }
  return null;
}

$('shortcut-add').addEventListener('click', () => openBox(null));
$('sb-cancel').addEventListener('click', closeBox);
$('sb-kind').addEventListener('change', showKind);
$('sb-system').addEventListener('change', () => {
  showKind();
  const icon = { mute: 'volume', lock: 'lock', focus: 'dnd' }[$('sb-system').value];
  box.icon = icon;
  swatches();
});
$('sb-app-find').addEventListener('input', fillChoices);
$('sb-app').addEventListener('change', () => {
  const chosen = ((shortcutState && shortcutState.apps) || []).find((a) => a.bundle === $('sb-app').value);
  if (!chosen) return;
  box.bundle = chosen.bundle;
  box.label = chosen.name;
  if (!$('sb-name').value) $('sb-name').value = chosen.name;
  box.icon = chosen.icon;
  box.color = chosen.color;
  swatches();
});
$('sb-pick').addEventListener('click', () => send({ 'shortcut-pick': true }));
$('sb-pick-folder').addEventListener('click', () => send({ 'shortcut-pick-folder': true }));
$('sb-keys').addEventListener('keydown', (event) => {
  event.preventDefault();
  if (['ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight'].includes(event.code)) return;
  box.combo = { code: event.code, command: event.metaKey, control: event.ctrlKey, option: event.altKey, shift: event.shiftKey };
  $('sb-keys').value = comboText(box.combo);
});
$('sb-save').addEventListener('click', () => {
  const action = draftAction();
  if (!action) { showError('할 일을 고르세요'); return; }
  if (action.kind === 'url' && !/^https?:\/\/[^\s/?#]+/i.test(action.url)) { showError('http 나 https 로 시작하는 주소만 열 수 있습니다'); return; }
  const draft = { name: $('sb-name').value.trim(), icon: box.icon, color: box.color, action };
  if (box.id !== null) draft.id = box.id;
  showError('');
  box.saving = true;
  send({ 'shortcut-save': draft });
});
$('shortcut-key-settings').addEventListener('click', () => send({ 'shortcut-key-settings': true }));
document.querySelector('[data-page="shortcuts"]').addEventListener('click', () => send({ 'shortcut-open': true }));

// 음악 앱 자동화 권한 안내. 까닭이 있을 때만 보인다.
function renderMusic(next) {
  const problem = next && next.problem;
  $('music-label').hidden = !problem;
  $('music-row').hidden = !problem;
  $('music-problem').textContent = problem || '';
}
$('music-settings').addEventListener('click', () => send({ 'music-settings': true }));
