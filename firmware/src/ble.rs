//! BLE: the touch service (see `protocol` and docs/BLE.md) on WCH's
//! closed-source peripheral stack (libCH572BLE_PERI.a,
//! "CH572_BLE_PERI_LIB_V1.2", from the openwch/ch570 EVT; `mise run
//! fw:wch` fetches it), and the poll loop and sleep that run under it.
//! Bindings follow CH572BLEPeri_LIB.h of the same release, and the
//! start-up and sleep follow WCH's examples (HAL/MCU.c CH57x_BLEInit and
//! HAL_Init, HAL/RTC.c HAL_TimeInit, HAL/SLEEP.c, Peripheral_Init).
//!
//! The stack runs partly in interrupts (its baseband and link-layer
//! handlers, which it installs as fast interrupts itself) and partly in
//! its task scheduler, TMOS, which the main loop runs ([`run`]).  The
//! firmware is one TMOS task: its events poll the touch wire, send
//! notifications and keep the clocks calibrated.  When TMOS has nothing
//! due it calls [`idle`], which sleeps the chip until the next event.
//!
//! Every callback here (GAP state, GATT reads and writes, task events,
//! idle) is called from TMOS_SystemProcess, so from the main loop, one at
//! a time; the library status callback alone may run in an interrupt, and
//! it only stores.

// The statics the library holds pointers to are `static mut`; they are
// borrowed through `&raw mut`, the form edition 2024 accepts, which
// clippy would collapse into the plain borrow the edition warns about.
#![allow(clippy::deref_addrof)]

use crate::hw;
use crate::protocol::{self, Diagnostics, TouchState};
use crate::sleep::{self, Plan};
use crate::touch::{DEFAULT_CONFIG, Detector, Event};
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering::Relaxed};

// ---- The library's interface (CH572BLEPeri_LIB.h V1.2) -------------------

const VER_FILE: &[u8] = b"CH572_BLE_PERI_LIB_V1.2";

#[repr(C)]
struct BleConfig {
    mem_addr: u32,
    mem_len: u16,
    snv_addr: u32,
    snv_block: u16,
    snv_num: u8,
    buf_number: u8,
    buf_max_len: u16,
    tx_num_event: u8,
    rx_num_event: u8,
    tx_power: u8,
    window_widening: u8,
    wait_window: u8,
    mac_addr: [u8; 6],
    srand_cb: Option<unsafe extern "C" fn() -> u32>,
    idle_cb: Option<unsafe extern "C" fn(u32) -> u32>,
    status_cb: Option<unsafe extern "C" fn(u8, u32)>,
    read_flash_cb: Option<unsafe extern "C" fn(u32, u32, *mut u32) -> u32>,
    write_flash_cb: Option<unsafe extern "C" fn(u32, u32, *mut u32) -> u32>,
}
const _: () = assert!(size_of::<BleConfig>() == 52);

#[repr(C)]
struct ClockConfig {
    get_clock_value: Option<unsafe extern "C" fn() -> u32>,
    clock_max_count: u32,
    clock_frequency: u16,
    clock_accuracy: u16,
    irq_enable: u8,
    set_pending_irq: Option<unsafe extern "C" fn()>,
}
const _: () = assert!(size_of::<ClockConfig>() == 20);

#[repr(C)]
#[derive(Clone, Copy)]
struct AttrType {
    len: u8,
    uuid: *const u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Attribute {
    kind: AttrType,
    permissions: u8,
    /// Assigned by the library when the service is registered.
    handle: u16,
    value: *mut u8,
}
const _: () = assert!(size_of::<Attribute>() == 16);

type ReadCb = unsafe extern "C" fn(u16, *mut Attribute, *mut u8, *mut u16, u16, u16, u8) -> u8;
type WriteCb = unsafe extern "C" fn(u16, *mut Attribute, *mut u8, u16, u16, u8) -> u8;
type AuthorizeCb = unsafe extern "C" fn(u16, *mut Attribute, u8) -> u8;

#[repr(C)]
struct ServiceCbs {
    read: Option<ReadCb>,
    write: Option<WriteCb>,
    authorize: Option<AuthorizeCb>,
}

/// One client's Client Characteristic Configuration value.
#[repr(C)]
#[derive(Clone, Copy)]
struct CharCfg {
    conn_handle: u16,
    value: u8,
}

#[repr(C)]
struct RoleCbs {
    state: Option<unsafe extern "C" fn(u32, *mut u8)>,
    rssi: Option<unsafe extern "C" fn(u16, i8)>,
    param_update: Option<unsafe extern "C" fn(u16, u16, u16, u16)>,
}

#[repr(C)]
struct BondCbs {
    passcode: Option<unsafe extern "C" fn()>,
    pair_state: Option<unsafe extern "C" fn()>,
    oob: Option<unsafe extern "C" fn()>,
}

#[repr(C)]
struct HandleValueNoti {
    handle: u16,
    len: u16,
    value: *mut u8,
}

/// gapEstLinkReqEvent_t, up to the field used.
#[repr(C)]
struct LinkEstablished {
    event: u8,
    status: u8,
    opcode: u8,
    addr_type: u8,
    addr: [u8; 6],
    conn_handle: u16,
}

unsafe extern "C" {
    static VER_LIB: [u8; VER_FILE.len()];
    static primaryServiceUUID: [u8; 2];
    static characterUUID: [u8; 2];
    static clientCharCfgUUID: [u8; 2];

    fn BLE_LibInit(cfg: *mut BleConfig) -> u8;
    fn BLE_RegInit();
    fn TMOS_TimerInit(cfg: *mut ClockConfig) -> u8;
    fn TMOS_TimerSet(cfg: *mut ClockConfig) -> u8;
    fn TMOS_SystemProcess();
    fn TMOS_GetSystemClock() -> u32;
    fn TMOS_ProcessEventRegister(cb: unsafe extern "C" fn(u8, u16) -> u16) -> u8;
    fn tmos_set_event(task: u8, event: u16) -> u8;
    fn tmos_start_task(task: u8, event: u16, time: u32) -> u8;
    fn tmos_stop_task(task: u8, event: u16) -> u8;
    fn tmos_msg_receive(task: u8) -> *mut u8;
    fn tmos_msg_deallocate(msg: *mut u8) -> u8;

    fn GAPRole_PeripheralInit() -> u8;
    fn GAPRole_PeripheralStartDevice(task: u8, bond: *mut BondCbs, role: *mut RoleCbs) -> u8;
    fn GAPRole_SetParameter(param: u16, len: u16, value: *mut c_void) -> u8;
    fn GAPRole_PeripheralConnParamUpdateReq(
        conn: u16,
        interval_min: u16,
        interval_max: u16,
        latency: u16,
        timeout: u16,
        task: u8,
    ) -> u8;
    fn GAP_SetParamValue(param: u16, value: u16) -> u8;
    fn GGS_AddService(services: u32) -> u8;
    fn GGS_SetParameter(param: u8, len: u8, value: *mut c_void) -> u8;
    fn GATTServApp_AddService(services: u32) -> u8;
    fn GATTServApp_RegisterService(
        attrs: *mut Attribute,
        count: u16,
        enc_key_size: u8,
        cbs: *mut ServiceCbs,
    ) -> u8;
    fn GATTServApp_InitCharCfg(conn: u16, table: *mut CharCfg);
    fn GATTServApp_ReadCharCfg(conn: u16, table: *mut CharCfg) -> u16;
    fn GATTServApp_ProcessCCCWriteReq(
        conn: u16,
        attr: *mut Attribute,
        value: *mut u8,
        len: u16,
        offset: u16,
        valid: u16,
    ) -> u8;
    fn GATT_Notification(conn: u16, noti: *mut HandleValueNoti, authenticated: u8) -> u8;
    fn GATT_bm_alloc(conn: u16, opcode: u8, size: u16, allocated: *mut u16, flag: u8) -> *mut u8;
    fn GATT_bm_free(msg: *mut HandleValueNoti, opcode: u8);
}

const SUCCESS: u8 = 0x00;
const SYS_EVENT_MSG: u16 = 0x8000;
const INVALID_CONNHANDLE: u16 = 0xFFFF;
const LL_TX_PWR_0_DBM: u8 = 0x12;
const GATT_ALL_SERVICES: u32 = 0xFFFF_FFFF;
const GATT_MAX_ENCRYPT_KEY_SIZE: u8 = 16;
const GATT_PERMIT_READ: u8 = 0x01;
const GATT_PERMIT_WRITE: u8 = 0x02;
const GATT_PROP_READ: u8 = 0x02;
const GATT_PROP_NOTIFY: u8 = 0x10;
const GATT_CLIENT_CFG_NOTIFY: u16 = 0x0001;
const ATT_HANDLE_VALUE_NOTI: u8 = 0x1B;
const ATT_ERR_ATTR_NOT_FOUND: u8 = 0x0A;
const ATT_ERR_ATTR_NOT_LONG: u8 = 0x0B;
const GAP_LINK_ESTABLISHED_EVENT: u8 = 0x05;
const GAP_LINK_TERMINATED_EVENT: u8 = 0x06;
const GAPROLE_STATE_ADV_MASK: u32 = 0xF;
const GAPROLE_CONNECTED: u32 = 4;
const GAPROLE_ADVERT_ENABLED: u16 = 0x305;
const GAPROLE_ADVERT_DATA: u16 = 0x306;
const GAPROLE_SCAN_RSP_DATA: u16 = 0x307;
const GAPROLE_MIN_CONN_INTERVAL: u16 = 0x311;
const GAPROLE_MAX_CONN_INTERVAL: u16 = 0x312;
const TGAP_DISC_ADV_INT_MIN: u16 = 2;
const TGAP_DISC_ADV_INT_MAX: u16 = 3;
const GGS_DEVICE_NAME_ATT: u8 = 0;

/// TMOS counts time in 625 us units (SYSTEM_TIME_MICROSEN).
const fn tmos_ms(ms: u32) -> u32 {
    ms * 1000 / 625
}

// ---- Configuration -------------------------------------------------------

/// Touch polls while no phone is connected: the chip sleeps in between.
pub const POLL_IDLE_MS: u32 = 200;
/// Touch polls while connected, for a quick push-to-talk: with two polls
/// to make a press, a press reaches the phone 50 to 100 ms after the
/// finger, plus up to one connection interval.
pub const POLL_CONNECTED_MS: u32 = 50;
/// Charges per touch poll, averaged.
const SAMPLES: u32 = 4;
/// Longest charge before a measurement counts as failed: 1 ms is 3.5 nF
/// through 1 MOhm, far beyond any wire.
const CHARGE_TIMEOUT_US: u32 = 1_000;
/// D1 lights while touched.  It draws current through R4 for as long as
/// the press lasts.
const TOUCH_LED: bool = true;
/// After reset the chip stays awake this long, with D2 lit, before it
/// first sleeps: a WCH-LinkE attaches to the running chip in that window
/// even when the firmware's sleep keeps it away later (power-cycle the
/// board, then flash within it).
pub const BOOT_AWAKE_MS: u32 = 5_000;

/// WCH's defaults for this chip (EVT HAL/include/CONFIG.h): the smallest
/// heap the stack accepts is 3 KB, and 27-byte packets give an ATT MTU of
/// 23.
const HEAP_SIZE: usize = 512 * 7;
const BUF_MAX_LEN: u16 = 27;
const BUF_NUMBER: u8 = 3;

/// The LSI's accuracy the stack plans its receive windows with, in ppm
/// (WCH's BLE_ClockConfig).
const LSI_ACCURACY_PPM: u16 = 2500;
/// Radio and LSI calibration: first after 500 ms, then every 2 minutes
/// (WCH's HAL_Init and BLE_CALIBRATION_PERIOD).
const CALIBRATION_FIRST_MS: u32 = 500;
const CALIBRATION_MS: u32 = 120_000;
/// Advertising interval, 200 ms in 0.625 ms units, to match the idle poll.
const ADV_INTERVAL: u16 = 320;
/// Connection parameters asked of the phone once connected (in 1.25 ms
/// units, latency in events, timeout in 10 ms): 30 to 50 ms, up to 4
/// skipped events when idle, 5 s supervision.  Asked after 4 s, once
/// service discovery has run at the phone's faster opening interval
/// (WCH's SBP_PARAM_UPDATE_DELAY).
const CONN_INTERVAL_MIN: u16 = 24;
const CONN_INTERVAL_MAX: u16 = 40;
const CONN_LATENCY: u16 = 4;
const CONN_TIMEOUT: u16 = 500;
const PARAM_UPDATE_DELAY_MS: u32 = 4_000;

/// TMOS events of the firmware's task.
const EVT_START: u16 = 0x0001;
const EVT_POLL: u16 = 0x0002;
const EVT_NOTIFY: u16 = 0x0004;
const EVT_CALIBRATE: u16 = 0x0008;
const EVT_PARAM_UPDATE: u16 = 0x0010;
const EVT_BOOT_DONE: u16 = 0x0020;

// ---- Memory the library holds on to --------------------------------------

#[repr(C, align(4))]
struct Heap([u8; HEAP_SIZE]);
static mut HEAP: Heap = Heap([0; HEAP_SIZE]);

static SERVICE_UUID: [u8; 16] = protocol::SERVICE;
static TOUCH_UUID: [u8; 16] = protocol::TOUCH;
static DEVICE_ID_UUID: [u8; 16] = protocol::DEVICE_ID;
static DIAGNOSTICS_UUID: [u8; 16] = protocol::DIAGNOSTICS;
static mut SERVICE_TYPE: AttrType = AttrType {
    len: 16,
    uuid: core::ptr::null(),
};
static mut TOUCH_PROPS: u8 = GATT_PROP_READ | GATT_PROP_NOTIFY;
static mut READ_PROPS: u8 = GATT_PROP_READ;
/// Stand-in value storage: the read callback answers every value from
/// the firmware's own state.
static mut NO_VALUE: [u8; 4] = [0; 4];
/// Room for more clients than the stack's one connection.
static mut TOUCH_CCC: [CharCfg; 4] = [CharCfg {
    conn_handle: INVALID_CONNHANDLE,
    value: 0,
}; 4];

const EMPTY: Attribute = Attribute {
    kind: AttrType {
        len: 0,
        uuid: core::ptr::null(),
    },
    permissions: 0,
    handle: 0,
    value: null_mut(),
};
const ATTR_COUNT: usize = 8;
/// Index of the touch value in [`ATTRS`]: its handle addresses
/// notifications.
const TOUCH_VALUE: usize = 2;
static mut ATTRS: [Attribute; ATTR_COUNT] = [EMPTY; ATTR_COUNT];

static mut SERVICE_CBS: ServiceCbs = ServiceCbs {
    read: Some(read_cb),
    write: Some(write_cb),
    authorize: None,
};
static mut ROLE_CBS: RoleCbs = RoleCbs {
    state: Some(state_cb),
    rssi: None,
    param_update: None,
};
static mut BOND_CBS: BondCbs = BondCbs {
    passcode: None,
    pair_state: None,
    oob: None,
};
static mut ADV_DATA: [u8; protocol::PAYLOAD_MAX] = [0; protocol::PAYLOAD_MAX];
static mut SCAN_RSP: [u8; protocol::PAYLOAD_MAX] = [0; protocol::PAYLOAD_MAX];
static mut DEVICE_NAME: [u8; protocol::NAME_LEN] = [0; protocol::NAME_LEN];

// ---- State ---------------------------------------------------------------

/// The firmware's state, used only from TMOS callbacks (see the module
/// doc), so never by two at once.
struct App {
    detector: Detector,
    state: TouchState,
    /// The touch value changed (or a client subscribed) and the client
    /// has not been told yet.
    notify_pending: bool,
    timeouts: u16,
    capture_fault: bool,
    device_id: [u8; 6],
}

struct Shared(UnsafeCell<App>);
// Single-threaded use, see App.
unsafe impl Sync for Shared {}

static APP: Shared = Shared(UnsafeCell::new(App {
    detector: Detector::new(DEFAULT_CONFIG),
    state: TouchState {
        touched: false,
        changes: 0,
    },
    notify_pending: false,
    timeouts: 0,
    capture_fault: false,
    device_id: [0; 6],
}));

/// The firmware's state, for the callback running now.
#[allow(clippy::mut_from_ref)]
fn app() -> &'static mut App {
    unsafe { &mut *APP.0.get() }
}

// Atomics only for Sync: the chip has no atomic instructions, and every
// update here is a plain load and store.
static TASK: AtomicU8 = AtomicU8::new(0xFF);
static CONN: AtomicU16 = AtomicU16::new(INVALID_CONNHANDLE);
static NOTIFY: AtomicBool = AtomicBool::new(false);
static SLEEP_ALLOWED: AtomicBool = AtomicBool::new(false);
static LSI_HZ: AtomicU32 = AtomicU32::new(32_000);
static WAKE_LEAD: AtomicU32 = AtomicU32::new(0);
static MIN_SLEEP: AtomicU32 = AtomicU32::new(0);
static LIB_CODE: AtomicU8 = AtomicU8::new(0);
static LIB_STATUS: AtomicU32 = AtomicU32::new(0);

// ---- Callbacks -----------------------------------------------------------

unsafe extern "C" fn srand() -> u32 {
    hw::ticks()
}

unsafe extern "C" fn lib_status(code: u8, status: u32) {
    LIB_CODE.store(code, Relaxed);
    LIB_STATUS.store(status, Relaxed);
}

fn clock_config(lsi_hz: u32) -> ClockConfig {
    ClockConfig {
        get_clock_value: Some(hw::rtc_count as unsafe extern "C" fn() -> u32),
        clock_max_count: sleep::RTC_MAX_COUNT,
        clock_frequency: lsi_hz.min(u16::MAX as u32) as u16,
        clock_accuracy: LSI_ACCURACY_PPM,
        irq_enable: 0,
        set_pending_irq: Some(hw::rtc_pend as unsafe extern "C" fn()),
    }
}

/// Measure the LSI and derive the sleep timing from it.
fn calibrate_lsi() -> u32 {
    let hz = hw::lsi_measure();
    LSI_HZ.store(hz, Relaxed);
    WAKE_LEAD.store(sleep::wake_lead(hz), Relaxed);
    MIN_SLEEP.store(sleep::min_sleep(hz), Relaxed);
    hz
}

/// TMOS's idle hook, WCH's CH57x_LowPower: `due` is the RTC count of the
/// next TMOS event.  Sleeps until shortly before it unless that is too
/// close.  The return value is informational: 0 slept, 2 too close, 3
/// stayed awake (boot window, or the alarm already fired).
unsafe extern "C" fn idle(due: u32) -> u32 {
    if !SLEEP_ALLOWED.load(Relaxed) {
        return 3;
    }
    let irq = hw::irq_save();
    let now = hw::rtc_count();
    let plan = sleep::plan(now, due, WAKE_LEAD.load(Relaxed), MIN_SLEEP.load(Relaxed));
    let Plan::Sleep { alarm } = plan else {
        hw::irq_restore(irq);
        return 2;
    };
    hw::rtc_alarm(alarm);
    hw::irq_restore(irq);
    if hw::rtc_fired() {
        return 3;
    }
    hw::sleep_now();
    0
}

unsafe extern "C" fn task_events(task: u8, events: u16) -> u16 {
    if events & SYS_EVENT_MSG != 0 {
        // GAP and GATT messages: nothing here needs them.
        let msg = unsafe { tmos_msg_receive(task) };
        if !msg.is_null() {
            unsafe { tmos_msg_deallocate(msg) };
        }
        return events ^ SYS_EVENT_MSG;
    }
    if events & EVT_START != 0 {
        unsafe {
            GAPRole_PeripheralStartDevice(task, &raw mut BOND_CBS, &raw mut ROLE_CBS);
        }
        return events ^ EVT_START;
    }
    if events & EVT_POLL != 0 {
        poll(task);
        return events ^ EVT_POLL;
    }
    if events & EVT_NOTIFY != 0 {
        send_notification(app());
        return events ^ EVT_NOTIFY;
    }
    if events & EVT_CALIBRATE != 0 {
        // WCH's HAL_REG_INIT_EVENT: radio calibration, then the LSI.
        unsafe { BLE_RegInit() };
        let mut clock = clock_config(calibrate_lsi());
        unsafe {
            TMOS_TimerSet(&mut clock);
            tmos_start_task(task, EVT_CALIBRATE, tmos_ms(CALIBRATION_MS));
        }
        return events ^ EVT_CALIBRATE;
    }
    if events & EVT_PARAM_UPDATE != 0 {
        let conn = CONN.load(Relaxed);
        if conn != INVALID_CONNHANDLE {
            unsafe {
                GAPRole_PeripheralConnParamUpdateReq(
                    conn,
                    CONN_INTERVAL_MIN,
                    CONN_INTERVAL_MAX,
                    CONN_LATENCY,
                    CONN_TIMEOUT,
                    task,
                );
            }
        }
        return events ^ EVT_PARAM_UPDATE;
    }
    if events & EVT_BOOT_DONE != 0 {
        hw::led_status(false);
        SLEEP_ALLOWED.store(true, Relaxed);
        return events ^ EVT_BOOT_DONE;
    }
    0
}

/// One touch poll: measure, feed the detector, report a change, and
/// schedule the next poll at the rate for the connection state.
fn poll(task: u8) {
    let connected = CONN.load(Relaxed) != INVALID_CONNHANDLE;
    let period = if connected {
        POLL_CONNECTED_MS
    } else {
        POLL_IDLE_MS
    };
    unsafe { tmos_start_task(task, EVT_POLL, tmos_ms(period)) };
    hw::watchdog_feed();
    let app = app();
    let m = hw::measure_touch(SAMPLES, CHARGE_TIMEOUT_US);
    app.capture_fault |= m.capture_fault;
    let Some(total) = m.total else {
        app.timeouts = app.timeouts.saturating_add(1);
        return;
    };
    let now_ms = (unsafe { TMOS_GetSystemClock() } as u64 * 625 / 1000) as u32;
    let Some(event) = app.detector.update(total / SAMPLES, now_ms) else {
        if app.notify_pending {
            send_notification(app);
        }
        return;
    };
    app.state.set(event == Event::Pressed);
    hw::led_touch(TOUCH_LED && app.state.touched);
    if connected {
        app.notify_pending = true;
        send_notification(app);
    } else {
        set_advertising(app.state);
    }
}

/// Tell a subscribed client the touch value.  A notification the stack
/// has no buffer for stays pending for the next poll, which then sends
/// the state as it is by then (`changes` shows what was skipped).
fn send_notification(app: &mut App) {
    let conn = CONN.load(Relaxed);
    if conn == INVALID_CONNHANDLE || !NOTIFY.load(Relaxed) {
        app.notify_pending = false;
        return;
    }
    let value = protocol::touch_value(app.state);
    let len = value.len() as u16;
    let buf = unsafe { GATT_bm_alloc(conn, ATT_HANDLE_VALUE_NOTI, len, null_mut(), 0) };
    if buf.is_null() {
        return;
    }
    unsafe { core::ptr::copy_nonoverlapping(value.as_ptr(), buf, value.len()) };
    let mut noti = HandleValueNoti {
        handle: unsafe { (*(&raw const ATTRS))[TOUCH_VALUE].handle },
        len,
        value: buf,
    };
    if unsafe { GATT_Notification(conn, &mut noti, 0) } != SUCCESS {
        unsafe { GATT_bm_free(&mut noti, ATT_HANDLE_VALUE_NOTI) };
        return;
    }
    app.notify_pending = false;
}

/// Put the touch state into the advertising data.  The role copies the
/// data and updates the controller's.
fn set_advertising(state: TouchState) {
    unsafe {
        let adv = &mut *(&raw mut ADV_DATA);
        let len = protocol::advertising(state, adv);
        GAPRole_SetParameter(GAPROLE_ADVERT_DATA, len as u16, adv.as_mut_ptr().cast());
    }
}

unsafe extern "C" fn state_cb(state: u32, event: *mut u8) {
    if event.is_null() {
        return;
    }
    let task = TASK.load(Relaxed);
    let opcode = unsafe { event.add(2).read() };
    if state & GAPROLE_STATE_ADV_MASK == GAPROLE_CONNECTED && opcode == GAP_LINK_ESTABLISHED_EVENT {
        let link = unsafe { &*(event as *const LinkEstablished) };
        if link.status == SUCCESS {
            CONN.store(link.conn_handle, Relaxed);
            unsafe {
                tmos_start_task(task, EVT_POLL, tmos_ms(POLL_CONNECTED_MS));
                tmos_start_task(task, EVT_PARAM_UPDATE, tmos_ms(PARAM_UPDATE_DELAY_MS));
            }
        }
    } else if opcode == GAP_LINK_TERMINATED_EVENT {
        let conn = CONN.load(Relaxed);
        unsafe { GATTServApp_InitCharCfg(conn, (&raw mut TOUCH_CCC).cast()) };
        CONN.store(INVALID_CONNHANDLE, Relaxed);
        NOTIFY.store(false, Relaxed);
        let app = app();
        app.notify_pending = false;
        // Advertise again, with the state as it is now, as WCH's
        // Peripheral_LinkTerminated does.
        set_advertising(app.state);
        let mut on: u8 = 1;
        unsafe {
            tmos_stop_task(task, EVT_PARAM_UPDATE);
            GAPRole_SetParameter(GAPROLE_ADVERT_ENABLED, 1, (&raw mut on).cast());
            tmos_start_task(task, EVT_POLL, tmos_ms(POLL_IDLE_MS));
        }
    }
}

unsafe extern "C" fn read_cb(
    _conn: u16,
    attr: *mut Attribute,
    value: *mut u8,
    len: *mut u16,
    offset: u16,
    max_len: u16,
    _method: u8,
) -> u8 {
    if offset != 0 {
        return ATT_ERR_ATTR_NOT_LONG;
    }
    let uuid = unsafe { (*attr).kind.uuid };
    let app = app();
    let mut buf = [0u8; protocol::DIAGNOSTICS_LEN];
    let n = if uuid == TOUCH_UUID.as_ptr() {
        let v = protocol::touch_value(app.state);
        buf[..v.len()].copy_from_slice(&v);
        v.len()
    } else if uuid == DEVICE_ID_UUID.as_ptr() {
        buf[..6].copy_from_slice(&app.device_id);
        6
    } else if uuid == DIAGNOSTICS_UUID.as_ptr() {
        buf = diagnostics(app).encode();
        buf.len()
    } else {
        // Declarations and the CCC descriptor the library answers itself.
        unsafe { *len = 0 };
        return ATT_ERR_ATTR_NOT_FOUND;
    };
    let n = n.min(max_len as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), value, n);
        *len = n as u16;
    }
    SUCCESS
}

fn diagnostics(app: &App) -> Diagnostics {
    Diagnostics {
        touched: app.state.touched,
        capture_fault: app.capture_fault,
        reading: app.detector.last(),
        baseline: app.detector.baseline(),
        threshold: app.detector.threshold(),
        timeouts: app.timeouts,
        lsi_hz: LSI_HZ.load(Relaxed).min(u16::MAX as u32) as u16,
        stack_unused: hw::stack_unused().min(u16::MAX as usize) as u16,
    }
}

unsafe extern "C" fn write_cb(
    conn: u16,
    attr: *mut Attribute,
    value: *mut u8,
    len: u16,
    offset: u16,
    _method: u8,
) -> u8 {
    let uuid = unsafe { (*attr).kind.uuid };
    if uuid != unsafe { clientCharCfgUUID.as_ptr() } {
        return ATT_ERR_ATTR_NOT_FOUND;
    }
    let status = unsafe {
        GATTServApp_ProcessCCCWriteReq(conn, attr, value, len, offset, GATT_CLIENT_CFG_NOTIFY)
    };
    if status == SUCCESS {
        let cfg = unsafe { GATTServApp_ReadCharCfg(conn, (&raw mut TOUCH_CCC).cast()) };
        let on = cfg & GATT_CLIENT_CFG_NOTIFY != 0;
        NOTIFY.store(on, Relaxed);
        if on {
            // A new subscriber hears the current state right away, after
            // the write's response.
            app().notify_pending = true;
            unsafe { tmos_set_event(TASK.load(Relaxed), EVT_NOTIFY) };
        }
    }
    status
}

// ---- Start-up and the main loop ------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    /// The linked library is not the release the bindings were written
    /// for.
    WrongLibrary,
    /// BLE_LibInit refused, with this code.
    LibInit(u8),
}

/// Start the stack: WCH's CH57x_BLEInit, HAL_Init (with HAL_SLEEP),
/// GAPRole_PeripheralInit and Peripheral_Init, in that order.  `mac` is
/// the chip's address, least significant byte first.  Call with
/// interrupts off; it enables them.
pub fn start(mac: [u8; 6]) -> Result<(), StartError> {
    if unsafe { &VER_LIB[..] } != VER_FILE {
        return Err(StartError::WrongLibrary);
    }
    app().device_id = protocol::display_address(mac);
    hw::ble_clocks_init();
    let lsi_hz = calibrate_lsi();
    let mut cfg = BleConfig {
        mem_addr: (&raw mut HEAP) as u32,
        mem_len: HEAP_SIZE as u16,
        // No SNV storage: no pairing, so no bonds to keep.
        snv_addr: 0,
        snv_block: 0,
        snv_num: 0,
        buf_number: BUF_NUMBER,
        buf_max_len: BUF_MAX_LEN,
        tx_num_event: 1,
        rx_num_event: 0,
        tx_power: LL_TX_PWR_0_DBM,
        window_widening: 120,
        wait_window: 0,
        mac_addr: mac,
        srand_cb: Some(srand),
        idle_cb: Some(idle),
        status_cb: Some(lib_status),
        read_flash_cb: None,
        write_flash_cb: None,
    };
    // The stack's handlers rely on the core's hardware register
    // stacking, and WCH starts it with interrupts on.
    hw::interrupts_init();
    hw::irq_enable();
    let status = unsafe { BLE_LibInit(&mut cfg) };
    if status != SUCCESS {
        return Err(StartError::LibInit(status));
    }
    unsafe {
        let task = TMOS_ProcessEventRegister(task_events);
        TASK.store(task, Relaxed);
        let mut clock = clock_config(lsi_hz);
        TMOS_TimerInit(&mut clock);
        hw::rtc_init();
        tmos_start_task(task, EVT_CALIBRATE, tmos_ms(CALIBRATION_FIRST_MS));
        GAPRole_PeripheralInit();
        peripheral_init(task, mac);
        tmos_start_task(task, EVT_POLL, tmos_ms(POLL_IDLE_MS));
        tmos_start_task(task, EVT_BOOT_DONE, tmos_ms(BOOT_AWAKE_MS));
    }
    Ok(())
}

/// Run TMOS forever, feeding the watchdog on each pass; the chip sleeps
/// inside TMOS_SystemProcess (through [`idle`]).  In RAM, as WCH's
/// Main_Circulation.
#[unsafe(link_section = ".highcode")]
#[inline(never)]
pub fn run() -> ! {
    loop {
        hw::watchdog_feed();
        unsafe { TMOS_SystemProcess() };
    }
}

unsafe fn peripheral_init(task: u8, mac: [u8; 6]) {
    unsafe {
        let name = &mut *(&raw mut DEVICE_NAME);
        *name = protocol::device_name(mac);
        let scan = &mut *(&raw mut SCAN_RSP);
        let scan_len = protocol::scan_response(name, scan);
        let mut on: u8 = 1;
        let mut min = CONN_INTERVAL_MIN;
        let mut max = CONN_INTERVAL_MAX;
        GAPRole_SetParameter(GAPROLE_ADVERT_ENABLED, 1, (&raw mut on).cast());
        GAPRole_SetParameter(
            GAPROLE_SCAN_RSP_DATA,
            scan_len as u16,
            scan.as_mut_ptr().cast(),
        );
        set_advertising(TouchState::default());
        GAPRole_SetParameter(GAPROLE_MIN_CONN_INTERVAL, 2, (&raw mut min).cast());
        GAPRole_SetParameter(GAPROLE_MAX_CONN_INTERVAL, 2, (&raw mut max).cast());
        GAP_SetParamValue(TGAP_DISC_ADV_INT_MIN, ADV_INTERVAL);
        GAP_SetParamValue(TGAP_DISC_ADV_INT_MAX, ADV_INTERVAL);

        GGS_AddService(GATT_ALL_SERVICES);
        GATTServApp_AddService(GATT_ALL_SERVICES);
        register_service();
        GGS_SetParameter(
            GGS_DEVICE_NAME_ATT,
            name.len() as u8,
            name.as_mut_ptr().cast(),
        );

        tmos_set_event(task, EVT_START);
    }
}

/// The attribute table: service declaration; touch (declaration, value,
/// CCC descriptor); device ID (declaration, value); diagnostics
/// (declaration, value).
unsafe fn register_service() {
    unsafe {
        (*(&raw mut SERVICE_TYPE)).uuid = SERVICE_UUID.as_ptr();
        let uuid16 = |uuid: &'static [u8; 2]| AttrType {
            len: 2,
            uuid: uuid.as_ptr(),
        };
        let uuid128 = |uuid: &'static [u8; 16]| AttrType {
            len: 16,
            uuid: uuid.as_ptr(),
        };
        let attr = |kind, permissions, value: *mut u8| Attribute {
            kind,
            permissions,
            handle: 0,
            value,
        };
        let no_value = (&raw mut NO_VALUE).cast::<u8>();
        let attrs = &mut *(&raw mut ATTRS);
        *attrs = [
            attr(
                uuid16(&primaryServiceUUID),
                GATT_PERMIT_READ,
                (&raw mut SERVICE_TYPE).cast(),
            ),
            attr(
                uuid16(&characterUUID),
                GATT_PERMIT_READ,
                &raw mut TOUCH_PROPS,
            ),
            attr(uuid128(&TOUCH_UUID), GATT_PERMIT_READ, no_value),
            attr(
                uuid16(&clientCharCfgUUID),
                GATT_PERMIT_READ | GATT_PERMIT_WRITE,
                (&raw mut TOUCH_CCC).cast(),
            ),
            attr(
                uuid16(&characterUUID),
                GATT_PERMIT_READ,
                &raw mut READ_PROPS,
            ),
            attr(uuid128(&DEVICE_ID_UUID), GATT_PERMIT_READ, no_value),
            attr(
                uuid16(&characterUUID),
                GATT_PERMIT_READ,
                &raw mut READ_PROPS,
            ),
            attr(uuid128(&DIAGNOSTICS_UUID), GATT_PERMIT_READ, no_value),
        ];
        GATTServApp_InitCharCfg(INVALID_CONNHANDLE, (&raw mut TOUCH_CCC).cast());
        GATTServApp_RegisterService(
            attrs.as_mut_ptr(),
            ATTR_COUNT as u16,
            GATT_MAX_ENCRYPT_KEY_SIZE,
            &raw mut SERVICE_CBS,
        );
    }
}
