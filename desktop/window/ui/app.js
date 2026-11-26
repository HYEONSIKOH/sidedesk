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

document.querySelectorAll('.menu-item').forEach((b) => b.addEventListener('click', () => show(b.dataset.page)));
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
