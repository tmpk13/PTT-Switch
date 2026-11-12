# BLE interface

What the ptt-switch board offers a phone over Bluetooth Low Energy: the
board reports one thing, whether its touch wire is touched. This is the
contract between the firmware (`firmware/`) and the Android app. A test
in `firmware/src/protocol.rs` checks that the UUIDs below match the
firmware's.

Status: the firmware builds and its logic is host-tested; nothing here has
run on a board yet.

## Finding the board

The board is a BLE peripheral advertising connectable, undirected
(ADV_IND) every 200 ms, with its **public** device address: the CH572D's
factory MAC address, which is unique per chip. That address is the
board's identity; Android's `BluetoothDevice.getAddress()` returns it
unchanged for a public address.

Advertising data (27 bytes):

| AD type | Content |
|-|-|
| `0x01` flags | `0x06`: LE General Discoverable, BR/EDR not supported |
| `0x07` complete 128-bit UUIDs | the service UUID below |
| `0xFF` manufacturer data | company ID `0xFFFF` (little-endian `FF FF`), then the 2-byte touch value below |

Scan response: `0x09` complete local name, `PTT-` and the last three
bytes of the address in upper-case hex: address `38:3B:26:0A:1B:2C`
advertises as `PTT-0A1B2C`.

Scan with a `ScanFilter` on the service UUID; it is in the advertising
packet itself, not only the scan response. The name is display text.

The manufacturer data carries the touch state while no phone is
connected, refreshed within one 200 ms poll of a change, so an app can
follow the switch from scan results alone. Android throttles and batches
scan results, so this path is slower and lossier than a connection; the
`changes` counter shows what was missed. While a phone is connected the
board does not advertise.

## The service

Service `dbc30001-a746-42c6-b521-fbfb2ba9d69e`:

| Characteristic | UUID | Properties | Value |
|-|-|-|-|
| Touch | `dbc30002-a746-42c6-b521-fbfb2ba9d69e` | read, notify | 2 bytes, below |
| Device ID | `dbc30003-a746-42c6-b521-fbfb2ba9d69e` | read | 6 bytes: the board's address, most significant byte first |
| Diagnostics | `dbc30004-a746-42c6-b521-fbfb2ba9d69e` | read | 20 bytes, for bench tuning; apps may ignore it |

No pairing or bonding: every characteristic is open to any central. One
connection at a time.

### Touch value

| Byte | Meaning |
|-|-|
| 0 | `1` touched, `0` not touched |
| 1 | `changes`: counts every change of byte 0, wrapping at 256 |

A jump of `changes` by more than 1 between two values means presses
happened in between (for example a press and release both within one
throttled scan interval). The count starts at 0 at power-up.

Subscribing (writing `0x0001` to the touch characteristic's CCC
descriptor, `0x2902`) makes the board notify the current value at once,
then on every change. Notifications follow the change by one to two touch
polls (50 ms each while connected) plus up to one connection interval.

### Device ID

The same 6 bytes as the device address, `38:3B:26:0A:1B:2C` reading as
`38 3B 26 0A 1B 2C`. It lets an app confirm which board it reached, and
names the board in logs where the address is not at hand.

### Diagnostics

Little-endian. For tuning the touch threshold with nRF Connect or a debug
screen; the layout may change with byte 0.

| Bytes | Field |
|-|-|
| 0 | format, `1` |
| 1 | flags: bit 0 touched, bit 1 capture fault (a charge timed out although the comparator had switched) |
| 2-5 | u32 latest charge time, in 10 ns counts (100 per microsecond) |
| 6-9 | u32 baseline: the untouched charge time the detector tracks |
| 10-13 | u32 threshold: counts above the baseline that make a touch |
| 14-15 | u16 measurements that timed out |
| 16-17 | u16 LSI frequency, Hz (the sleep clock, nominally 32 kHz) |
| 18-19 | u16 stack bytes never used since reset |

## Timing and power

| State | Touch polls | Radio |
|-|-|-|
| First 5 s after power-up | every 200 ms | advertising; the chip stays awake (LED D2 lit) so a debug probe can attach |
| Not connected | every 200 ms | advertising every 200 ms, asleep in between |
| Connected | every 50 ms | asleep between polls and connection events |

A press takes two polls above the threshold, a release two polls below
half of it. A touch held longer than 2 minutes is released and the
baseline restarts from it (taken for a changed surrounding, such as a wet
enclosure, rather than a press), so a covered wire cannot hold the
switch on.

Four seconds after connecting, the board asks for a 30 to 50 ms
connection interval with a peripheral latency of 4 and a 5 s supervision
timeout. An app that wants the lowest latency can call
`requestConnectionPriority(CONNECTION_PRIORITY_HIGH)`; the board accepts
what the phone decides.
