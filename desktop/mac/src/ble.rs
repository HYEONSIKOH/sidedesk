//! 맥 CoreBluetooth 로 SideDesk 기기를 찾고 L2CAP 채널로 붙는다.
//! CoreBluetooth 를 부르는 `unsafe` 는 이 파일에만 둔다.

use std::collections::{HashMap, HashSet};
use std::ptr::NonNull;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, Message, define_class, msg_send};
use objc2_core_bluetooth::{
    CBAdvertisementDataLocalNameKey, CBAdvertisementDataServiceDataKey, CBCentralManager,
    CBCentralManagerDelegate, CBCentralManagerScanOptionAllowDuplicatesKey, CBL2CAPChannel,
    CBManagerState, CBPeripheral, CBPeripheralDelegate, CBUUID,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSStreamStatus,
    NSString,
};
use sidedesk_core::{BEACON_LEN, BLE_SERVICE, parse_service_data};
use sidedesk_domain::{Conn, Link, Phone, PhoneLinks, PortError, Received};

/// 이만큼 알림 신호가 안 보이면 목록에서 뺀다(붙어 있는 기기는 빼지 않는다).
const GONE: Duration = Duration::from_secs(5);
/// 다른 앱의 블루투스 연결이 같은 기기를 잡고 있으면 붙기가 끝나지 않는다.
const CONNECT_LIMIT: Duration = Duration::from_secs(10);
const CHANNEL_LIMIT: Duration = Duration::from_secs(5);
const READ_WAIT: Duration = Duration::from_millis(250);
const WRITE_LIMIT: Duration = Duration::from_secs(5);
const PREFIX: &str = "ble:";

/// 한 스레드가 쥐고 있다가 다른 스레드로 넘기기만 하는 CoreBluetooth 객체(기기, 채널, 스트림).
/// 동시에 두 스레드가 쓰지 않는다.
struct Sendable<T>(T);
// SAFETY: 위 설명대로 한 번에 한 스레드만 쓴다. 넘기는 것만 허용하고 Sync 는 붙이지 않는다.
unsafe impl<T> Send for Sendable<T> {}

struct Seen {
    peripheral: Sendable<Retained<CBPeripheral>>,
    psm: u16,
    beacon: [u8; BEACON_LEN],
    name: Option<String>,
    last: Instant,
}

enum Step {
    Connected,
    Failed(String),
    Channel(Sendable<Retained<CBL2CAPChannel>>),
    Disconnected,
}

#[derive(Default)]
struct State {
    /// 맥 블루투스를 쓸 수 없는 이유
    problem: Option<String>,
    seen: HashMap<String, Seen>,
    connected: HashSet<String>,
    /// 기기 id 마다 지금 기다리는 연결 번호와 받는 쪽
    waiters: HashMap<String, (u64, Sender<Step>)>,
    tickets: u64,
    /// 붙는 중인 수. 0 일 때만 찾기를 켠다.
    connecting: usize,
    version: u64,
}

#[derive(Default)]
struct Hub {
    state: Mutex<State>,
    changed: Condvar,
}

impl Hub {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|lock| lock.into_inner())
    }

    fn bump(&self, state: &mut State) {
        state.version += 1;
        self.changed.notify_all();
    }

    fn step(&self, id: &str, step: Step) {
        if let Some((_, waiter)) = self.lock().waiters.get(id) {
            let _ = waiter.send(step);
        }
    }

    /// 이 연결 번호가 걸어 둔 대기자만 지운다.
    fn forget_waiter(&self, id: &str, ticket: u64) {
        let mut state = self.lock();
        if state
            .waiters
            .get(id)
            .is_some_and(|(mine, _)| *mine == ticket)
        {
            state.waiters.remove(id);
        }
    }
}

fn peripheral_id(peripheral: &CBPeripheral) -> String {
    // SAFETY: 읽기 전용 속성
    let uuid = unsafe { peripheral.identifier() };
    format!("{PREFIX}{}", uuid.UUIDString())
}

fn service_uuid() -> Retained<CBUUID> {
    // SAFETY: 올바른 UUID 문자열
    unsafe { CBUUID::UUIDWithString(&NSString::from_str(BLE_SERVICE)) }
}

define_class!(
    // SAFETY: NSObject 는 하위 클래스 조건이 없고, 이 클래스는 Drop 을 구현하지 않는다.
    #[unsafe(super(NSObject))]
    #[name = "SideDeskBleDelegate"]
    #[ivars = Arc<Hub>]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl CBCentralManagerDelegate for Delegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn did_update_state(&self, central: &CBCentralManager) {
            let hub = self.ivars();
            // SAFETY: 읽기 전용 속성
            let power = unsafe { central.state() };
            let problem = match power {
                CBManagerState::PoweredOn => None,
                CBManagerState::PoweredOff => Some("맥의 블루투스가 꺼져 있습니다"),
                CBManagerState::Unauthorized => {
                    Some("맥 설정에서 SideDesk 의 블루투스 권한을 허용해 주세요")
                }
                CBManagerState::Unsupported => Some("이 맥은 블루투스 LE 를 지원하지 않습니다"),
                _ => Some("맥의 블루투스를 준비하는 중입니다"),
            };
            let mut state = hub.lock();
            state.problem = problem.map(str::to_owned);
            if problem.is_some() {
                state.seen.clear();
            }
            let idle = state.connecting == 0;
            hub.bump(&mut state);
            drop(state);
            if power == CBManagerState::PoweredOn && idle {
                scan(central);
            }
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn did_discover(
            &self,
            _central: &CBCentralManager,
            peripheral: &CBPeripheral,
            advertisement: &NSDictionary<NSString, AnyObject>,
            _rssi: &NSNumber,
        ) {
            let Some(data) = service_data(advertisement) else {
                return;
            };
            let Some((psm, beacon)) = parse_service_data(&data) else {
                return;
            };
            let name = local_name(advertisement);
            let id = peripheral_id(peripheral);
            let hub = self.ivars();
            let mut state = hub.lock();
            let fresh = match state.seen.get(&id) {
                Some(old) => old.psm != psm || old.beacon != beacon || (name.is_some() && old.name != name),
                None => true,
            };
            let name = name.or_else(|| state.seen.get(&id).and_then(|old| old.name.clone()));
            state.seen.insert(
                id,
                Seen {
                    peripheral: Sendable(peripheral.retain()),
                    psm,
                    beacon,
                    name,
                    last: Instant::now(),
                },
            );
            if fresh {
                hub.bump(&mut state);
            }
        }

        #[unsafe(method(centralManager:didConnectPeripheral:))]
        fn did_connect(&self, _central: &CBCentralManager, peripheral: &CBPeripheral) {
            self.ivars().step(&peripheral_id(peripheral), Step::Connected);
        }

        #[unsafe(method(centralManager:didFailToConnectPeripheral:error:))]
        fn did_fail(
            &self,
            _central: &CBCentralManager,
            peripheral: &CBPeripheral,
            error: Option<&NSError>,
        ) {
            let reason = error
                .map(|error| error.localizedDescription().to_string())
                .unwrap_or_else(|| "블루투스로 붙지 못했습니다".to_owned());
            self.ivars().step(&peripheral_id(peripheral), Step::Failed(reason));
        }

        #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
        fn did_disconnect(
            &self,
            _central: &CBCentralManager,
            peripheral: &CBPeripheral,
            _error: Option<&NSError>,
        ) {
            let id = peripheral_id(peripheral);
            let hub = self.ivars();
            hub.step(&id, Step::Disconnected);
            let mut state = hub.lock();
            if state.connected.remove(&id) {
                hub.bump(&mut state);
            }
        }
    }

    unsafe impl CBPeripheralDelegate for Delegate {
        #[unsafe(method(peripheral:didOpenL2CAPChannel:error:))]
        fn did_open_channel(
            &self,
            peripheral: &CBPeripheral,
            channel: Option<&CBL2CAPChannel>,
            error: Option<&NSError>,
        ) {
            let step = match (channel, error) {
                (Some(channel), None) => Step::Channel(Sendable(channel.retain())),
                (_, error) => Step::Failed(
                    error
                        .map(|error| error.localizedDescription().to_string())
                        .unwrap_or_else(|| "블루투스 채널을 열지 못했습니다".to_owned()),
                ),
            };
            self.ivars().step(&peripheral_id(peripheral), step);
        }
    }
);

impl Delegate {
    fn new(hub: Arc<Hub>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(hub);
        // SAFETY: NSObject 의 init
        unsafe { msg_send![super(this), init] }
    }
}

fn scan(central: &CBCentralManager) {
    let services = NSArray::from_retained_slice(&[service_uuid()]);
    let yes: Retained<AnyObject> = NSNumber::new_bool(true).into();
    // SAFETY: 키는 CoreBluetooth 상수, 값은 NSNumber
    let options = unsafe {
        NSDictionary::from_retained_objects(&[CBCentralManagerScanOptionAllowDuplicatesKey], &[yes])
    };
    // SAFETY: 서비스 목록과 옵션 사전의 형이 문서대로다.
    unsafe { central.scanForPeripheralsWithServices_options(Some(&services), Some(&options)) };
}

fn service_data(advertisement: &NSDictionary<NSString, AnyObject>) -> Option<Vec<u8>> {
    // SAFETY: CoreBluetooth 상수 키
    let value = advertisement.objectForKey(unsafe { CBAdvertisementDataServiceDataKey })?;
    let table = value.downcast::<NSDictionary>().ok()?;
    let ours = service_uuid();
    for key in table.allKeys().iter() {
        let Ok(uuid) = key.downcast::<CBUUID>() else {
            continue;
        };
        if uuid.isEqual(Some(&ours)) {
            let data = table.objectForKey(&uuid)?.downcast::<NSData>().ok()?;
            return Some(data.to_vec());
        }
    }
    None
}

fn local_name(advertisement: &NSDictionary<NSString, AnyObject>) -> Option<String> {
    // SAFETY: CoreBluetooth 상수 키
    let value = advertisement.objectForKey(unsafe { CBAdvertisementDataLocalNameKey })?;
    Some(value.downcast::<NSString>().ok()?.to_string())
}

/// 관리자와 대리자. 관리자 메서드는 전용 큐 안에서만 부른다(콜백도 그 큐로 온다).
struct Manager {
    central: Retained<CBCentralManager>,
    delegate: Retained<Delegate>,
    queue: DispatchRetained<DispatchQueue>,
}

// SAFETY: 관리자와 대리자는 `on_queue` 를 거쳐 전용 직렬 큐에서만 쓴다. 큐가 한 번에 하나씩 돌린다.
unsafe impl Send for Manager {}
// SAFETY: 같은 이유
unsafe impl Sync for Manager {}

impl Manager {
    fn on_queue(&self, work: impl FnOnce(&CBCentralManager, &Delegate)) {
        struct Job<F>(F);
        // SAFETY: 작업은 exec_sync 가 끝나기 전에 큐에서 한 번 돌고 끝난다. 그동안 부른 스레드는 기다린다.
        unsafe impl<F> Send for Job<F> {}
        let job = Job(|| work(&self.central, &self.delegate));
        self.queue.exec_sync(move || {
            let job = job;
            (job.0)()
        });
    }

    /// 붙기를 시작한다. 처음 붙는 것이면 찾기를 멈춘다(찾는 중에는 붙기가 끝나지 않는 일이 있다).
    fn begin(&self, hub: &Hub, peripheral: &CBPeripheral) {
        let first = {
            let mut state = hub.lock();
            state.connecting += 1;
            state.connecting == 1
        };
        let peripheral = Sendable(peripheral);
        self.on_queue(|central, delegate| {
            let peripheral = &peripheral;
            // SAFETY: 이 관리자가 찾은 기기에 대리자를 두고 붙는다.
            unsafe {
                if first {
                    central.stopScan();
                }
                peripheral
                    .0
                    .setDelegate(Some(ProtocolObject::from_ref(delegate)));
                central.connectPeripheral_options(peripheral.0, None);
            }
        });
    }

    /// 붙기가 끝났다(성공이든 실패든). 붙는 중인 것이 없으면 찾기를 다시 켠다.
    fn end(&self, hub: &Hub) {
        let idle = {
            let mut state = hub.lock();
            state.connecting = state.connecting.saturating_sub(1);
            state.connecting == 0 && state.problem.is_none()
        };
        if idle {
            self.on_queue(|central, _| scan(central));
        }
    }

    fn cancel(&self, peripheral: &CBPeripheral) {
        let peripheral = Sendable(peripheral);
        self.on_queue(|central, _| {
            let peripheral = &peripheral;
            // SAFETY: 이 관리자가 붙인 기기를 놓는다. 그래야 기기가 다시 알림 신호를 낸다.
            unsafe { central.cancelPeripheralConnection(peripheral.0) };
        });
    }

    fn open_channel(&self, peripheral: &CBPeripheral, psm: u16) {
        let peripheral = Sendable(peripheral);
        self.on_queue(|_, _| {
            let peripheral = &peripheral;
            // SAFETY: 붙은 기기의 채널을 연다.
            unsafe { peripheral.0.openL2CAPChannel(psm) };
        });
    }
}

/// 맥 블루투스 링크. 만들면 곧바로 찾기 시작한다.
pub struct BleLinks {
    hub: Arc<Hub>,
    manager: Arc<Manager>,
}

impl BleLinks {
    pub fn start() -> Self {
        let hub = Arc::new(Hub::default());
        let delegate = Delegate::new(Arc::clone(&hub));
        let queue = DispatchQueue::new(
            "io.github.hyeonsikoh.sidedesk.ble",
            DispatchQueueAttr::SERIAL,
        );
        // SAFETY: 대리자와 큐는 관리자와 함께 Manager 에 남는다.
        let central = unsafe {
            CBCentralManager::initWithDelegate_queue(
                CBCentralManager::alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
                Some(&queue),
            )
        };
        Self {
            hub,
            manager: Arc::new(Manager {
                central,
                delegate,
                queue,
            }),
        }
    }

    /// 오래 안 보인 기기는 지우고, 최근에 보였거나 붙어 있는 기기만 목록으로 낸다.
    fn list(state: &mut State) -> Result<Vec<Phone>, PortError> {
        if let Some(problem) = &state.problem {
            return Err(PortError::new(problem.clone()));
        }
        let connected = &state.connected;
        state
            .seen
            .retain(|id, seen| seen.last.elapsed() < GONE * 6 || connected.contains(id));
        let mut phones: Vec<Phone> = state
            .seen
            .iter()
            .filter(|(id, seen)| seen.last.elapsed() < GONE || connected.contains(*id))
            .map(|(id, seen)| Phone {
                id: id.clone(),
                model: seen.name.clone(),
                blocked: None,
                link: Link::Ble,
                beacon: Some(seen.beacon),
            })
            .collect();
        phones.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(phones)
    }

    /// 원하는 단계를 기다린다. 이전 연결의 늦은 끊김 소식은 붙기 전이면 버린다.
    fn wait(steps: &Receiver<Step>, limit: Duration, connected: bool) -> Option<Step> {
        let until = Instant::now() + limit;
        loop {
            let left = until.checked_duration_since(Instant::now())?;
            match steps.recv_timeout(left) {
                Ok(Step::Disconnected) if !connected => continue,
                Ok(step) => return Some(step),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn give_up(&self, peripheral: &CBPeripheral, id: &str, ticket: u64, reason: &str) -> PortError {
        self.manager.cancel(peripheral);
        self.hub.forget_waiter(id, ticket);
        let mut state = self.hub.lock();
        if state.connected.remove(id) {
            self.hub.bump(&mut state);
        }
        PortError::new(reason)
    }
}

impl PhoneLinks for BleLinks {
    fn link(&self) -> Link {
        Link::Ble
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        Self::list(&mut self.hub.lock())
    }

    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError> {
        let mut last: Option<Vec<Phone>> = None;
        loop {
            let mut state = self.hub.lock();
            let phones = Self::list(&mut state)?;
            if last.as_ref() != Some(&phones) {
                drop(state);
                changed(phones.clone());
                last = Some(phones);
                continue;
            }
            // 바뀐 것이 없어도 1초마다 다시 보아 사라진 기기를 뺀다.
            let _ = self
                .hub
                .changed
                .wait_timeout(state, Duration::from_secs(1))
                .unwrap_or_else(|lock| lock.into_inner());
        }
    }

    fn connect(&self, id: &str) -> Result<Box<dyn Conn>, PortError> {
        let (tx, steps) = mpsc::channel();
        let (peripheral, psm, ticket) = {
            let mut state = self.hub.lock();
            let seen = state
                .seen
                .get(id)
                .ok_or_else(|| PortError::new("블루투스에서 기기가 보이지 않습니다"))?;
            let peripheral = seen.peripheral.0.clone();
            let psm = seen.psm;
            state.tickets += 1;
            let ticket = state.tickets;
            state.waiters.insert(id.to_owned(), (ticket, tx));
            (peripheral, psm, ticket)
        };
        self.manager.begin(&self.hub, &peripheral);
        let result = self.finish_connect(id, &peripheral, psm, ticket, steps);
        self.manager.end(&self.hub);
        result
    }
}

impl BleLinks {
    fn finish_connect(
        &self,
        id: &str,
        peripheral: &Retained<CBPeripheral>,
        psm: u16,
        ticket: u64,
        steps: Receiver<Step>,
    ) -> Result<Box<dyn Conn>, PortError> {
        match Self::wait(&steps, CONNECT_LIMIT, false) {
            Some(Step::Connected) => {}
            Some(Step::Failed(reason)) => return Err(self.give_up(peripheral, id, ticket, &reason)),
            _ => {
                return Err(self.give_up(
                    peripheral,
                    id,
                    ticket,
                    "이 기기와 다른 블루투스 연결이 있어 붙지 못했습니다. 그 앱을 끄고 다시 시도하세요",
                ));
            }
        }
        {
            let mut state = self.hub.lock();
            state.connected.insert(id.to_owned());
            self.hub.bump(&mut state);
        }
        self.manager.open_channel(peripheral, psm);
        let channel = match Self::wait(&steps, CHANNEL_LIMIT, true) {
            Some(Step::Channel(channel)) => channel,
            Some(Step::Failed(reason)) => return Err(self.give_up(peripheral, id, ticket, &reason)),
            _ => {
                return Err(self.give_up(
                    peripheral,
                    id,
                    ticket,
                    "블루투스 채널을 열지 못했습니다",
                ));
            }
        };
        // SAFETY: 열린 채널의 스트림
        let (input, output) = unsafe { (channel.0.inputStream(), channel.0.outputStream()) };
        let (Some(input), Some(output)) = (input, output) else {
            return Err(self.give_up(peripheral, id, ticket, "블루투스 채널에 통로가 없습니다"));
        };
        input.open();
        output.open();
        Ok(Box::new(BleConn {
            id: id.to_owned(),
            ticket,
            hub: Arc::clone(&self.hub),
            manager: Arc::clone(&self.manager),
            peripheral: Sendable(peripheral.clone()),
            _channel: channel,
            input: Sendable(input),
            output: Sendable(output),
            steps,
        }))
    }
}

struct BleConn {
    id: String,
    ticket: u64,
    hub: Arc<Hub>,
    manager: Arc<Manager>,
    peripheral: Sendable<Retained<CBPeripheral>>,
    _channel: Sendable<Retained<CBL2CAPChannel>>,
    input: Sendable<Retained<objc2_foundation::NSInputStream>>,
    output: Sendable<Retained<objc2_foundation::NSOutputStream>>,
    steps: Receiver<Step>,
}

impl BleConn {
    fn broken(&self) -> bool {
        if matches!(self.steps.try_recv(), Ok(Step::Disconnected)) {
            return true;
        }
        let status = self.input.0.streamStatus();
        status == NSStreamStatus::AtEnd
            || status == NSStreamStatus::Closed
            || status == NSStreamStatus::Error
    }
}

impl Conn for BleConn {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<Received, PortError> {
        let start = Instant::now();
        loop {
            if self.input.0.hasBytesAvailable() {
                // SAFETY: buffer 는 길이만큼 쓸 수 있다.
                let read = unsafe {
                    self.input.0.read_maxLength(
                        NonNull::new(buffer.as_mut_ptr()).expect("버퍼"),
                        buffer.len(),
                    )
                };
                return Ok(match read {
                    n if n > 0 => Received::Bytes(n as usize),
                    0 => Received::Closed,
                    _ => return Err(PortError::new("블루투스에서 읽지 못했습니다")),
                });
            }
            if self.broken() {
                return Ok(Received::Closed);
            }
            if start.elapsed() >= READ_WAIT {
                return Ok(Received::Nothing);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), PortError> {
        let start = Instant::now();
        let mut sent = 0;
        while sent < bytes.len() {
            if self.broken() {
                return Err(PortError::new("블루투스 연결이 끊겼습니다"));
            }
            if self.output.0.hasSpaceAvailable() {
                let rest = &bytes[sent..];
                // SAFETY: rest 는 길이만큼 읽을 수 있다.
                let wrote = unsafe {
                    self.output
                        .0
                        .write_maxLength(NonNull::from(rest).cast(), rest.len())
                };
                if wrote < 0 {
                    return Err(PortError::new("블루투스로 보내지 못했습니다"));
                }
                sent += wrote as usize;
                continue;
            }
            if start.elapsed() > WRITE_LIMIT {
                return Err(PortError::new("블루투스로 보내는 데 너무 오래 걸립니다"));
            }
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
}

impl Drop for BleConn {
    fn drop(&mut self) {
        self.input.0.close();
        self.output.0.close();
        self.manager.cancel(&self.peripheral.0);
        self.hub.forget_waiter(&self.id, self.ticket);
        let mut state = self.hub.lock();
        if state.connected.remove(&self.id) {
            self.hub.bump(&mut state);
        }
    }
}
