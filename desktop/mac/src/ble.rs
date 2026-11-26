//! 맥 CoreBluetooth 로 SideDesk 기기를 찾고 L2CAP 채널로 붙는다.
//! CoreBluetooth 를 부르는 `unsafe` 는 이 파일에만 둔다.
//! 아이폰은 알림 신호에 서비스 데이터를 못 실으므로, 한 번 붙어 GATT 특성에서 같은 값을 읽는다.

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
    CBCentralManagerDelegate, CBCentralManagerScanOptionAllowDuplicatesKey, CBCharacteristic,
    CBL2CAPChannel, CBManagerState, CBPeripheral, CBPeripheralDelegate, CBService, CBUUID,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSStreamStatus,
    NSString,
};
use sidedesk_core::{BEACON_LEN, BLE_INFO, BLE_SERVICE, parse_service_data};
use sidedesk_domain::{Conn, Link, Phone, PhoneLinks, PortError, Received};

/// 이만큼 알림 신호가 안 보이면 목록에서 뺀다(붙어 있는 기기는 빼지 않는다).
const GONE: Duration = Duration::from_secs(5);
/// 붙기를 시작하려면 이 안에 알림 신호가 보였어야 한다.
const FRESH: Duration = Duration::from_secs(2);
/// 다른 앱의 블루투스 연결이 같은 기기를 잡고 있으면 붙기가 끝나지 않는다.
const CONNECT_LIMIT: Duration = Duration::from_secs(10);
const CHANNEL_LIMIT: Duration = Duration::from_secs(5);
const READ_WAIT: Duration = Duration::from_millis(250);
/// 읽을 것이 없을 때 다시 볼 간격. 짧으면 연결 하나가 CPU 를 깨워 먹는다.
const READ_POLL: Duration = Duration::from_millis(20);
const WRITE_LIMIT: Duration = Duration::from_secs(5);
const PREFIX: &str = "ble:";
/// GATT 찾기와 읽기 한 단계의 제한
const READ_LIMIT: Duration = Duration::from_secs(5);
/// 아이폰 정보 읽기가 실패하면 이만큼 뒤에 다시 읽는다.
const PROBE_RETRY: Duration = Duration::from_secs(30);
/// 서비스 데이터 없이만 이만큼 보인 기기를 아이폰으로 보고 읽는다.
const SETTLE: Duration = Duration::from_secs(1);
/// 붙기가 읽는 중인 기기를 기다리는 한도
const PROBE_WAIT: Duration = Duration::from_secs(15);

/// 한 스레드가 쥐고 있다가 다른 스레드로 넘기는 CoreBluetooth 객체(기기, 채널, 스트림).
/// 빌린 참조를 `Manager::on_queue` 로 잠깐 넘기기도 한다(부른 쪽이 끝날 때까지 기다린다).
/// 동시에 두 스레드가 쓰지 않는다.
struct Sendable<T>(T);
// SAFETY: 위 설명대로 한 번에 한 스레드만 쓴다. 넘기는 것만 허용하고 Sync 는 붙이지 않는다.
unsafe impl<T> Send for Sendable<T> {}

/// PSM 과 알림 신호 지문
type Info = (u16, [u8; BEACON_LEN]);

struct Seen {
    peripheral: Sendable<Retained<CBPeripheral>>,
    /// 모르면(아직 안 읽은 아이폰) 목록에 내지 않는다.
    info: Option<Info>,
    /// 서비스 데이터 없이 알리는 기기(아이폰). 붙을 때마다 특성을 다시 읽는다.
    gatt: bool,
    /// 마지막으로 읽기에 실패한 때
    failed: Option<Instant>,
    /// 처음 본 때. 안드로이드도 이름만 실린 응답이 먼저 올 수 있어, 조금 지켜본 뒤에 아이폰으로 본다.
    since: Instant,
    name: Option<String>,
    last: Instant,
}

enum Step {
    Connected,
    Failed(String),
    Channel(Sendable<Retained<CBL2CAPChannel>>),
    Disconnected,
    Services,
    Characteristics,
    Value(Vec<u8>),
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
    /// 정보를 읽으려고 붙어 있는 기기
    probing: HashSet<String>,
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

fn info_uuid() -> Retained<CBUUID> {
    // SAFETY: 올바른 UUID 문자열
    unsafe { CBUUID::UUIDWithString(&NSString::from_str(BLE_INFO)) }
}

fn failure(error: Option<&NSError>, otherwise: Step) -> Step {
    match error {
        Some(error) => Step::Failed(error.localizedDescription().to_string()),
        None => otherwise,
    }
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
            // 찾기는 SideDesk 서비스로 걸렀으므로, 서비스 데이터가 없으면 아이폰이다.
            let carried = match service_data(advertisement) {
                Some(data) => match parse_service_data(&data) {
                    Some(info) => Some(info),
                    None => return,
                },
                None => None,
            };
            let name = local_name(advertisement);
            let id = peripheral_id(peripheral);
            let hub = self.ivars();
            let mut state = hub.lock();
            let old = state.seen.get(&id);
            // 서비스 데이터가 없는 알림 신호(아이폰, 또는 이름만 실린 응답)는 읽어 둔 값을 남긴다.
            let (info, gatt, failed) = match (carried, old) {
                (Some(info), _) => (Some(info), false, None),
                (None, Some(old)) => (old.info, old.gatt, old.failed),
                (None, None) => (None, true, None),
            };
            let fresh = match old {
                Some(old) => old.info != info || (name.is_some() && old.name != name),
                None => true,
            };
            let name = name.or_else(|| old.and_then(|old| old.name.clone()));
            let since = old.map_or_else(Instant::now, |old| old.since);
            state.seen.insert(
                id,
                Seen {
                    peripheral: Sendable(peripheral.retain()),
                    info,
                    gatt,
                    failed,
                    since,
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

        #[unsafe(method(peripheral:didDiscoverServices:))]
        fn did_discover_services(&self, peripheral: &CBPeripheral, error: Option<&NSError>) {
            self.ivars()
                .step(&peripheral_id(peripheral), failure(error, Step::Services));
        }

        #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
        fn did_discover_characteristics(
            &self,
            peripheral: &CBPeripheral,
            _service: &CBService,
            error: Option<&NSError>,
        ) {
            self.ivars()
                .step(&peripheral_id(peripheral), failure(error, Step::Characteristics));
        }

        #[unsafe(method(peripheral:didUpdateValueForCharacteristic:error:))]
        fn did_update_value(
            &self,
            peripheral: &CBPeripheral,
            characteristic: &CBCharacteristic,
            error: Option<&NSError>,
        ) {
            // SAFETY: 읽기 전용 속성
            let value = unsafe { characteristic.value() }
                .map(|data| data.to_vec())
                .unwrap_or_default();
            self.ivars()
                .step(&peripheral_id(peripheral), failure(error, Step::Value(value)));
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
    /// 큐 안에서 돌린다. 큐에서 도는 콜백 안에서는 부르지 않는다(exec_sync 를 다시 들어가면 멈춘다).
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
        let peripheral = Sendable(peripheral);
        self.on_queue(|central, delegate| {
            let peripheral = &peripheral;
            // 수 세기와 찾기 멈춤을 같은 큐 작업에서 해 순서가 뒤집히지 않게 한다.
            let first = {
                let mut state = hub.lock();
                state.connecting += 1;
                state.connecting == 1
            };
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
        self.on_queue(|central, _| {
            let idle = {
                let mut state = hub.lock();
                state.connecting = state.connecting.saturating_sub(1);
                state.connecting == 0 && state.problem.is_none()
            };
            if idle {
                scan(central);
            }
        });
    }

    fn cancel(&self, peripheral: &CBPeripheral) {
        let peripheral = Sendable(peripheral);
        self.on_queue(|central, _| {
            let peripheral = &peripheral;
            // SAFETY: 이 관리자가 붙인 기기를 놓는다. 그래야 기기가 다시 알림 신호를 낸다.
            unsafe { central.cancelPeripheralConnection(peripheral.0) };
        });
    }

    fn discover_services(&self, peripheral: &CBPeripheral) {
        let peripheral = Sendable(peripheral);
        self.on_queue(|_, _| {
            let peripheral = &peripheral;
            let services = NSArray::from_retained_slice(&[service_uuid()]);
            // SAFETY: 붙은 기기에서 우리 서비스만 찾는다.
            unsafe { peripheral.0.discoverServices(Some(&services)) };
        });
    }

    /// 찾은 우리 서비스. 큐 안에서 읽는다.
    fn our_service(&self, peripheral: &CBPeripheral) -> Option<Sendable<Retained<CBService>>> {
        let peripheral = Sendable(peripheral);
        let mut found = None;
        self.on_queue(|_, _| {
            let peripheral = &peripheral;
            let ours = service_uuid();
            // SAFETY: 읽기 전용 속성
            let services = unsafe { peripheral.0.services() };
            found = services
                .and_then(|list| {
                    list.iter().find(|service| {
                        // SAFETY: 읽기 전용 속성
                        unsafe { service.UUID() }.isEqual(Some(&ours))
                    })
                })
                .map(Sendable);
        });
        found
    }

    fn discover_info(&self, peripheral: &CBPeripheral, service: &CBService) {
        let both = Sendable((peripheral, service));
        self.on_queue(|_, _| {
            let both = &both;
            let wanted = NSArray::from_retained_slice(&[info_uuid()]);
            // SAFETY: 찾은 서비스에서 정보 특성만 찾는다.
            unsafe {
                both.0
                    .0
                    .discoverCharacteristics_forService(Some(&wanted), both.0.1)
            };
        });
    }

    /// 정보 특성을 찾았으면 읽기를 시작하고 true
    fn read_info(&self, peripheral: &CBPeripheral, service: &CBService) -> bool {
        let both = Sendable((peripheral, service));
        let mut started = false;
        self.on_queue(|_, _| {
            let both = &both;
            let wanted = info_uuid();
            // SAFETY: 읽기 전용 속성
            let characteristics = unsafe { both.0.1.characteristics() };
            let Some(info) = characteristics.and_then(|list| {
                list.iter().find(|item| {
                    // SAFETY: 읽기 전용 속성
                    unsafe { item.UUID() }.isEqual(Some(&wanted))
                })
            }) else {
                return;
            };
            // SAFETY: 붙은 기기의 읽기 전용 특성을 읽는다.
            unsafe { both.0.0.readValueForCharacteristic(&info) };
            started = true;
        });
        started
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
        let manager = Arc::new(Manager {
            central,
            delegate,
            queue,
        });
        let (probe_hub, probe_manager) = (Arc::clone(&hub), Arc::clone(&manager));
        thread::Builder::new()
            .name("sidedesk-ble-probe".into())
            .spawn(move || probe_forever(&probe_hub, &probe_manager))
            .expect("블루투스 읽기 스레드를 띄우지 못함");
        Self { hub, manager }
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
            .filter_map(|(id, seen)| {
                let (_, beacon) = seen.info?;
                Some(Phone {
                    id: id.clone(),
                    model: seen.name.clone(),
                    blocked: None,
                    link: Link::Ble,
                    beacon: Some(beacon),
                })
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
        let (peripheral, info, gatt, ticket) = {
            let mut state = self.hub.lock();
            // 정보를 읽으려고 붙어 있는 중이면 끝날 때까지 기다린다(대기자가 하나뿐이라 겹치면 단계가 엉킨다).
            let until = Instant::now() + PROBE_WAIT;
            while state.probing.contains(id) {
                let Some(left) = until.checked_duration_since(Instant::now()) else {
                    return Err(PortError::new("블루투스 기기 정보를 읽는 중입니다"));
                };
                state = self
                    .hub
                    .changed
                    .wait_timeout(state, left)
                    .unwrap_or_else(|lock| lock.into_inner())
                    .0;
            }
            // 주소가 바뀌어 사라진 기기에 10초 동안 매달리지 않는다(살아 있는 기기는 자주 보인다).
            let seen = state
                .seen
                .get(id)
                .filter(|seen| seen.last.elapsed() < FRESH)
                .ok_or_else(|| PortError::new("블루투스에서 기기가 보이지 않습니다"))?;
            let info = seen
                .info
                .ok_or_else(|| PortError::new("블루투스 기기 정보를 아직 읽지 못했습니다"))?;
            let peripheral = seen.peripheral.0.clone();
            let gatt = seen.gatt;
            state.tickets += 1;
            let ticket = state.tickets;
            state.waiters.insert(id.to_owned(), (ticket, tx));
            (peripheral, info, gatt, ticket)
        };
        self.manager.begin(&self.hub, &peripheral);
        let result = self.finish_connect(id, &peripheral, info, gatt, ticket, steps);
        self.manager.end(&self.hub);
        result
    }
}

impl BleLinks {
    fn finish_connect(
        &self,
        id: &str,
        peripheral: &Retained<CBPeripheral>,
        (psm, beacon): Info,
        gatt: bool,
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
        // 아이폰은 앱을 다시 켜면 PSM 이 바뀌므로 붙을 때마다 다시 읽는다.
        let psm = if gatt {
            match read_info(&self.manager, peripheral, &steps) {
                Ok((psm, mark)) if mark == beacon => psm,
                Ok(_) => {
                    return Err(self.give_up(peripheral, id, ticket, "기기의 키가 바뀌었습니다"));
                }
                Err(reason) => return Err(self.give_up(peripheral, id, ticket, &reason)),
            }
        } else {
            psm
        };
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
            thread::sleep(READ_POLL);
        }
    }

    /// 큰 그림도 보낼 수 있게, 시간 제한은 마지막으로 조금이라도 보낸 뒤부터 센다.
    fn send(&mut self, bytes: &[u8]) -> Result<(), PortError> {
        let mut progress = Instant::now();
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
                if wrote > 0 {
                    progress = Instant::now();
                }
                continue;
            }
            if progress.elapsed() > WRITE_LIMIT {
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

/// 붙은 기기에서 서비스, 정보 특성을 차례로 찾아 읽는다.
fn read_info(
    manager: &Manager,
    peripheral: &CBPeripheral,
    steps: &Receiver<Step>,
) -> Result<Info, String> {
    manager.discover_services(peripheral);
    match BleLinks::wait(steps, READ_LIMIT, true) {
        Some(Step::Services) => {}
        Some(Step::Failed(reason)) => return Err(reason),
        _ => return Err("블루투스 서비스를 찾지 못했습니다".to_owned()),
    }
    let service = manager
        .our_service(peripheral)
        .ok_or_else(|| "블루투스 서비스가 없습니다".to_owned())?;
    manager.discover_info(peripheral, &service.0);
    match BleLinks::wait(steps, READ_LIMIT, true) {
        Some(Step::Characteristics) => {}
        Some(Step::Failed(reason)) => return Err(reason),
        _ => return Err("블루투스 기기 정보를 찾지 못했습니다".to_owned()),
    }
    if !manager.read_info(peripheral, &service.0) {
        return Err("블루투스 기기 정보가 없습니다".to_owned());
    }
    match BleLinks::wait(steps, READ_LIMIT, true) {
        Some(Step::Value(bytes)) => {
            parse_service_data(&bytes).ok_or_else(|| "블루투스 기기 정보가 이상합니다".to_owned())
        }
        Some(Step::Failed(reason)) => Err(reason),
        _ => Err("블루투스 기기 정보를 읽지 못했습니다".to_owned()),
    }
}

/// 아직 정보를 모르는 아이폰 하나를 고른다. 다른 붙기가 진행 중이면 고르지 않는다.
fn next_probe(state: &State) -> Option<(String, Retained<CBPeripheral>)> {
    if state.problem.is_some() || state.connecting > 0 {
        return None;
    }
    state
        .seen
        .iter()
        .find(|(id, seen)| {
            seen.gatt
                && seen.info.is_none()
                && seen.last.elapsed() < FRESH
                && seen.since.elapsed() >= SETTLE
                && !state.waiters.contains_key(*id)
                && seen.failed.is_none_or(|at| at.elapsed() >= PROBE_RETRY)
        })
        .map(|(id, seen)| (id.clone(), seen.peripheral.0.clone()))
}

/// 아이폰 정보 읽기. 붙고, 읽고, 바로 놓는다(그래야 아이폰이 다시 알림 신호를 낸다).
fn probe_forever(hub: &Hub, manager: &Manager) {
    loop {
        let (id, peripheral, ticket, steps) = {
            let mut state = hub.lock();
            let Some((id, peripheral)) = next_probe(&state) else {
                let _ = hub
                    .changed
                    .wait_timeout(state, Duration::from_secs(1))
                    .unwrap_or_else(|lock| lock.into_inner());
                continue;
            };
            let (tx, steps) = mpsc::channel();
            state.tickets += 1;
            let ticket = state.tickets;
            state.waiters.insert(id.clone(), (ticket, tx));
            state.probing.insert(id.clone());
            (id, peripheral, ticket, steps)
        };
        manager.begin(hub, &peripheral);
        let read = match BleLinks::wait(&steps, CONNECT_LIMIT, false) {
            Some(Step::Connected) => read_info(manager, &peripheral, &steps),
            Some(Step::Failed(reason)) => Err(reason),
            _ => Err("블루투스로 붙지 못했습니다".to_owned()),
        };
        manager.cancel(&peripheral);
        hub.forget_waiter(&id, ticket);
        manager.end(hub);
        let mut state = hub.lock();
        state.probing.remove(&id);
        if let Some(seen) = state.seen.get_mut(&id) {
            match read {
                Ok(info) => {
                    seen.info = Some(info);
                    seen.failed = None;
                }
                Err(_) => seen.failed = Some(Instant::now()),
            }
        }
        hub.bump(&mut state);
    }
}
