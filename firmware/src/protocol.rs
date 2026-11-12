//! What the board shows a phone over BLE: the GATT service's UUIDs, the
//! values of its characteristics and the advertising payloads.  `ble`
//! serves them; docs/BLE.md describes them for the app, and a test checks
//! that its UUIDs match these.

/// Bytes of a 128-bit UUID in the order BLE carries them (little-endian),
/// from its usual text form.
const fn uuid128(text: &[u8; 36]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut i = 0;
    let mut n = 0;
    while i < 36 {
        if text[i] != b'-' {
            let nibble = match text[i] {
                b'0'..=b'9' => text[i] - b'0',
                b'a'..=b'f' => text[i] - b'a' + 10,
                b'A'..=b'F' => text[i] - b'A' + 10,
                _ => panic!("not a hex digit"),
            };
            // Text runs most significant first; BLE wants it reversed.
            let byte = 15 - n / 2;
            out[byte] |= if n % 2 == 0 { nibble << 4 } else { nibble };
            n += 1;
        }
        i += 1;
    }
    assert!(n == 32);
    out
}

/// The service and its characteristics share one random base UUID and
/// differ in the last digit of the first group.
pub const SERVICE_TEXT: &[u8; 36] = b"dbc30001-a746-42c6-b521-fbfb2ba9d69e";
pub const TOUCH_TEXT: &[u8; 36] = b"dbc30002-a746-42c6-b521-fbfb2ba9d69e";
pub const DEVICE_ID_TEXT: &[u8; 36] = b"dbc30003-a746-42c6-b521-fbfb2ba9d69e";
pub const DIAGNOSTICS_TEXT: &[u8; 36] = b"dbc30004-a746-42c6-b521-fbfb2ba9d69e";

pub const SERVICE: [u8; 16] = uuid128(SERVICE_TEXT);
/// Read and notify: [`touch_value`].
pub const TOUCH: [u8; 16] = uuid128(TOUCH_TEXT);
/// Read: the chip's unique ID, [`display_address`].
pub const DEVICE_ID: [u8; 16] = uuid128(DEVICE_ID_TEXT);
/// Read: [`Diagnostics::encode`], for tuning the touch detection.
pub const DIAGNOSTICS: [u8; 16] = uuid128(DIAGNOSTICS_TEXT);

/// Bluetooth SIG company identifier 0xFFFF, reserved for testing and
/// internal use, on the manufacturer data that carries the touch state.
pub const COMPANY_ID: u16 = 0xFFFF;

/// Legacy advertising and scan response payloads carry 31 bytes each.
pub const PAYLOAD_MAX: usize = 31;

const AD_FLAGS: u8 = 0x01;
const AD_UUID128_COMPLETE: u8 = 0x07;
const AD_NAME_COMPLETE: u8 = 0x09;
const AD_MANUFACTURER: u8 = 0xFF;
/// LE General Discoverable, BR/EDR not supported.
const FLAGS: u8 = 0x02 | 0x04;

/// The touch state as the board reports it, in the touch characteristic
/// and in the advertising data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchState {
    pub touched: bool,
    /// Counts every change of `touched`, wrapping at 256, so a reader that
    /// missed a short press between two reads (or two scan reports) sees
    /// the count jump by 2.
    pub changes: u8,
}

impl TouchState {
    pub fn set(&mut self, touched: bool) {
        if touched != self.touched {
            self.touched = touched;
            self.changes = self.changes.wrapping_add(1);
        }
    }
}

/// The touch characteristic: [touched (0 or 1), changes].
pub fn touch_value(state: TouchState) -> [u8; 2] {
    [state.touched as u8, state.changes]
}

/// Advertising data: the flags, the service UUID (scanners filter on it)
/// and manufacturer data [company ID, touched, changes], so a phone sees
/// the touch state without connecting.  Returns the length used in `out`.
pub fn advertising(state: TouchState, out: &mut [u8; PAYLOAD_MAX]) -> usize {
    out[..3].copy_from_slice(&[2, AD_FLAGS, FLAGS]);
    out[3] = 17;
    out[4] = AD_UUID128_COMPLETE;
    out[5..21].copy_from_slice(&SERVICE);
    out[21] = 5;
    out[22] = AD_MANUFACTURER;
    out[23..25].copy_from_slice(&COMPANY_ID.to_le_bytes());
    out[25..27].copy_from_slice(&touch_value(state));
    27
}

/// Scan response: the complete name.  Returns the length used in `out`.
pub fn scan_response(name: &[u8], out: &mut [u8; PAYLOAD_MAX]) -> usize {
    let len = name.len().min(PAYLOAD_MAX - 2);
    out[0] = len as u8 + 1;
    out[1] = AD_NAME_COMPLETE;
    out[2..2 + len].copy_from_slice(&name[..len]);
    2 + len
}

/// The chip's MAC address as the BLE stack takes it (least significant
/// byte first, as stored in the chip's info area) turned into the order
/// it is written in, most significant first: AA:BB:CC:DD:EE:FF is
/// [0xAA, ..., 0xFF].  Android's BluetoothDevice.getAddress() shows the
/// same, since the board advertises this as its public address.
pub fn display_address(mac: [u8; 6]) -> [u8; 6] {
    let mut out = mac;
    out.reverse();
    out
}

pub const NAME_LEN: usize = 10;

/// The advertised name, "PTT-" and the last three bytes of the address in
/// hex, so boards are told apart in a scan list without any setup.
pub fn device_name(mac: [u8; 6]) -> [u8; NAME_LEN] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = *b"PTT-000000";
    let addr = display_address(mac);
    for (i, byte) in addr[3..].iter().enumerate() {
        out[4 + 2 * i] = HEX[(byte >> 4) as usize];
        out[5 + 2 * i] = HEX[(byte & 0x0F) as usize];
    }
    out
}

/// Measurement details for tuning on the bench (nRF Connect reads them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub touched: bool,
    /// A measurement timed out while the comparator already read high: the
    /// timer never captured the crossing, so the capture path is suspect.
    pub capture_fault: bool,
    /// Latest charge time and the detector's view of it, in system clock
    /// counts (100 per microsecond).
    pub reading: u32,
    pub baseline: u32,
    pub threshold: u32,
    /// Measurements that timed out (the wire never reached the reference).
    pub timeouts: u16,
    /// The LSI frequency as last measured, Hz.
    pub lsi_hz: u16,
    /// Stack bytes never used since reset.
    pub stack_unused: u16,
}

pub const DIAGNOSTICS_LEN: usize = 20;
/// First byte of the diagnostics value; changes with its layout.
pub const DIAGNOSTICS_FORMAT: u8 = 1;

impl Diagnostics {
    /// [format, flags, reading, baseline, threshold (u32 LE each),
    /// timeouts, lsi_hz, stack_unused (u16 LE each)]; flags bit 0 touched,
    /// bit 1 capture fault.
    pub fn encode(&self) -> [u8; DIAGNOSTICS_LEN] {
        let mut out = [0u8; DIAGNOSTICS_LEN];
        out[0] = DIAGNOSTICS_FORMAT;
        out[1] = self.touched as u8 | (self.capture_fault as u8) << 1;
        out[2..6].copy_from_slice(&self.reading.to_le_bytes());
        out[6..10].copy_from_slice(&self.baseline.to_le_bytes());
        out[10..14].copy_from_slice(&self.threshold.to_le_bytes());
        out[14..16].copy_from_slice(&self.timeouts.to_le_bytes());
        out[16..18].copy_from_slice(&self.lsi_hz.to_le_bytes());
        out[18..20].copy_from_slice(&self.stack_unused.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_are_little_endian() {
        assert_eq!(
            SERVICE,
            [
                0x9e, 0xd6, 0xa9, 0x2b, 0xfb, 0xfb, 0x21, 0xb5, 0xc6, 0x42, 0x46, 0xa7, 0x01, 0x00,
                0xc3, 0xdb
            ]
        );
        assert_eq!(TOUCH[12], 0x02);
        assert_eq!(DEVICE_ID[12], 0x03);
        assert_eq!(DIAGNOSTICS[12], 0x04);
        assert_eq!(TOUCH[..12], SERVICE[..12]);
    }

    #[test]
    fn docs_list_the_same_uuids() {
        let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/BLE.md"))
            .expect("docs/BLE.md");
        for text in [SERVICE_TEXT, TOUCH_TEXT, DEVICE_ID_TEXT, DIAGNOSTICS_TEXT] {
            let text = std::str::from_utf8(text).unwrap();
            assert!(doc.contains(text), "docs/BLE.md lacks {text}");
        }
        assert!(doc.contains("0xFFFF"));
    }

    #[test]
    fn touch_state_counts_changes_only() {
        let mut s = TouchState::default();
        s.set(false);
        assert_eq!(touch_value(s), [0, 0]);
        s.set(true);
        s.set(true);
        assert_eq!(touch_value(s), [1, 1]);
        s.set(false);
        assert_eq!(touch_value(s), [0, 2]);
        s.changes = 255;
        s.set(true);
        assert_eq!(touch_value(s), [1, 0]);
    }

    #[test]
    fn advertising_carries_flags_service_and_state() {
        let mut out = [0u8; PAYLOAD_MAX];
        let state = TouchState {
            touched: true,
            changes: 7,
        };
        let n = advertising(state, &mut out);
        assert_eq!(n, 27);
        assert_eq!(out[..5], [2, 0x01, 0x06, 17, 0x07]);
        assert_eq!(out[5..21], SERVICE);
        assert_eq!(out[21..27], [5, 0xFF, 0xFF, 0xFF, 1, 7]);
    }

    #[test]
    fn scan_response_names_the_board() {
        let mut out = [0u8; PAYLOAD_MAX];
        let n = scan_response(b"PTT-0A1B2C", &mut out);
        assert_eq!(&out[..n], b"\x0b\x09PTT-0A1B2C");
    }

    #[test]
    fn name_and_id_follow_the_written_address() {
        // Stored least significant byte first: the address is
        // 38:3B:26:0A:1B:2C.
        let mac = [0x2C, 0x1B, 0x0A, 0x26, 0x3B, 0x38];
        assert_eq!(display_address(mac), [0x38, 0x3B, 0x26, 0x0A, 0x1B, 0x2C]);
        assert_eq!(&device_name(mac), b"PTT-0A1B2C");
    }

    #[test]
    fn diagnostics_layout() {
        let d = Diagnostics {
            touched: true,
            capture_fault: true,
            reading: 0x0102_0304,
            baseline: 300,
            threshold: 30,
            timeouts: 2,
            lsi_hz: 32_100,
            stack_unused: 1_000,
        };
        let b = d.encode();
        assert_eq!(b[..2], [1, 3]);
        assert_eq!(b[2..6], [4, 3, 2, 1]);
        assert_eq!(u32::from_le_bytes(b[6..10].try_into().unwrap()), 300);
        assert_eq!(u32::from_le_bytes(b[10..14].try_into().unwrap()), 30);
        assert_eq!(u16::from_le_bytes(b[14..16].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(b[16..18].try_into().unwrap()), 32_100);
        assert_eq!(u16::from_le_bytes(b[18..20].try_into().unwrap()), 1_000);
    }
}
