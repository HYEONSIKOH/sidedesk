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

/// CoreBluetooth 객체를 스레드 사이로 넘긴다.
/// CoreBluetooth 는 어느 스레드에서 불러도 되고, 콜백은 우리가 정한 큐로만 온다.
struct Shared<T>(T);
// SAFETY: 위 설명대로 CoreBluetooth 객체는 스레드에 묶이지 않는다. 동시에 바꾸는 상태는 Mutex 로 감싼다.
unsafe impl<T> Send for Shared<T> {}
// SAFETY: 같은 이유
unsafe impl<T> Sync for Shared<T> {}

struct Seen {
    peripheral: Shared<Retained<CBPeripheral>>,
    psm: u16,
    beacon: [u8; BEACON_LEN],
    name: Option<String>,
    last: Instant,
}

enum Step {
    Connected,
    Failed(String),
    Channel(Shared<Retained<CBL2CAPChannel>>),
    Disconnected,
}

#[derive(Default)]
struct State {
    /// 맥 블루투스를 쓸 수 없는 이유
    problem: Option<String>,
    seen: HashMap<String, Seen>,
    connected: HashSet<String>,
    waiters: HashMap<String, Sender<Step>>,
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
        if let Some(waiter) = self.lock().waiters.get(id) {
            let _ = waiter.send(step);
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
            hub.bump(&mut state);
            drop(state);
            if power == CBManagerState::PoweredOn {
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
                    peripheral: Shared(peripheral.retain()),
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
                (Some(channel), None) => Step::Channel(Shared(channel.retain())),
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

/// 맥 블루투스 링크. 만들면 곧바로 찾기 시작한다.
pub struct BleLinks {
    hub: Arc<Hub>,
    central: Shared<Retained<CBCentralManager>>,
    delegate: Shared<Retained<Delegate>>,
    _queue: Shared<DispatchRetained<DispatchQueue>>,
}

impl BleLinks {
    pub fn start() -> Self {
        let hub = Arc::new(Hub::default());
        let delegate = Delegate::new(Arc::clone(&hub));
        let queue = DispatchQueue::new(
            "io.github.hyeonsikoh.sidedesk.ble",
            DispatchQueueAttr::SERIAL,
        );
        // SAFETY: 대리자와 큐는 이 구조체가 살아 있는 동안 함께 산다.
        let central = unsafe {
            CBCentralManager::initWithDelegate_queue(
                CBCentralManager::alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
                Some(&queue),
            )
        };
        Self {
            hub,
            central: Shared(central),
            delegate: Shared(delegate),
            _queue: Shared(queue),
        }
    }

    fn list(state: &State) -> Result<Vec<Phone>, PortError> {
        if let Some(problem) = &state.problem {
            return Err(PortError::new(problem.clone()));
        }
        let mut phones: Vec<Phone> = state
            .seen
            .iter()
            .filter(|(id, seen)| seen.last.elapsed() < GONE || state.connected.contains(*id))
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

    fn wait(steps: &Receiver<Step>, limit: Duration) -> Result<Step, RecvTimeoutError> {
        steps.recv_timeout(limit)
    }

    fn give_up(&self, peripheral: &CBPeripheral, id: &str, reason: String) -> PortError {
        // SAFETY: 붙던 기기를 놓는다.
        unsafe { self.central.0.cancelPeripheralConnection(peripheral) };
        scan(&self.central.0);
        let mut state = self.hub.lock();
        state.waiters.remove(id);
        state.connected.remove(id);
        PortError::new(reason)
    }
}

impl PhoneLinks for BleLinks {
    fn link(&self) -> Link {
        Link::Ble
    }

    fn attached(&self) -> Result<Vec<Phone>, PortError> {
        Self::list(&self.hub.lock())
    }

    fn watch(&self, changed: &mut dyn FnMut(Vec<Phone>)) -> Result<(), PortError> {
        let mut last: Option<Vec<Phone>> = None;
        loop {
            let state = self.hub.lock();
            let phones = Self::list(&state)?;
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
        let (peripheral, psm) = {
            let mut state = self.hub.lock();
            let seen = state
                .seen
                .get(id)
                .ok_or_else(|| PortError::new("블루투스에서 기기가 보이지 않습니다"))?;
            let found = (seen.peripheral.0.clone(), seen.psm);
            state.waiters.insert(id.to_owned(), tx);
            found
        };
        // 찾는 중에는 붙기가 끝나지 않는 일이 있어, 붙는 동안 찾기를 멈춘다.
        // SAFETY: 이 큐의 관리자와 이 관리자가 찾은 기기
        unsafe {
            self.central.0.stopScan();
            peripheral.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate.0)));
            self.central.0.connectPeripheral_options(&peripheral, None);
        }
        match Self::wait(&steps, CONNECT_LIMIT) {
            Ok(Step::Connected) => {}
            Ok(Step::Failed(reason)) => return Err(self.give_up(&peripheral, id, reason)),
            Ok(_) | Err(_) => {
                return Err(self.give_up(
                    &peripheral,
                    id,
                    "이 기기와 다른 블루투스 연결이 있어 붙지 못했습니다. 그 앱을 끄고 다시 시도하세요"
                        .to_owned(),
                ));
            }
        }
        self.hub.lock().connected.insert(id.to_owned());
        // SAFETY: 붙은 기기의 채널을 연다.
        unsafe { peripheral.openL2CAPChannel(psm) };
        let channel = match Self::wait(&steps, CHANNEL_LIMIT) {
            Ok(Step::Channel(channel)) => channel,
            Ok(Step::Failed(reason)) => return Err(self.give_up(&peripheral, id, reason)),
            _ => {
                return Err(self.give_up(
                    &peripheral,
                    id,
                    "블루투스 채널을 열지 못했습니다".to_owned(),
                ));
            }
        };
        scan(&self.central.0);
        // SAFETY: 열린 채널의 스트림
        let (input, output) = unsafe { (channel.0.inputStream(), channel.0.outputStream()) };
        let (Some(input), Some(output)) = (input, output) else {
            return Err(self.give_up(
                &peripheral,
                id,
                "블루투스 채널에 통로가 없습니다".to_owned(),
            ));
        };
        input.open();
        output.open();
        Ok(Box::new(BleConn {
            id: id.to_owned(),
            hub: Arc::clone(&self.hub),
            central: Shared(self.central.0.clone()),
            peripheral: Shared(peripheral),
            _channel: channel,
            input: Shared(input),
            output: Shared(output),
            steps,
        }))
    }
}

struct BleConn {
    id: String,
    hub: Arc<Hub>,
    central: Shared<Retained<CBCentralManager>>,
    peripheral: Shared<Retained<CBPeripheral>>,
    _channel: Shared<Retained<CBL2CAPChannel>>,
    input: Shared<Retained<objc2_foundation::NSInputStream>>,
    output: Shared<Retained<objc2_foundation::NSOutputStream>>,
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
        // SAFETY: 이 연결이 붙인 기기를 놓는다. 그래야 기기가 다시 알림 신호를 낸다.
        unsafe {
            self.central
                .0
                .cancelPeripheralConnection(&self.peripheral.0)
        };
        let mut state = self.hub.lock();
        state.waiters.remove(&self.id);
        if state.connected.remove(&self.id) {
            self.hub.bump(&mut state);
        }
    }
}
