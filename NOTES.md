# Notes

## Origin of the CH572D circuit

- The schematic started as a copy of the CH572D USB bridge sheet
  (`usb_bridge.kicad_sch`) of
  [AGRV2KL48-fpga](https://github.com/tmpk13/AGRV2KL48-fpga), taken from its
  working tree on 2026-11-11 (hand-arranged in Eeschema after commit
  8e86637; its bridge tests passed on that state). Its CH572D symbol,
  footprint, 32 MHz crystal footprint and 3D model came along.
- Dropped from that sheet: the AG32 bridge nets (SWD, NRST, BOOT0, UART0),
  their series resistors, and the notes about the bridge firmware.
  References were renumbered for this board (CH572D is U1).
- Kept, with the reasons it was built that way:
  - Single 3.3 V supply mode: V5 tied to VDD33, 2.2 uF + 100 nF at VDD33
    (datasheet power-pin requirement).
  - Y1 has no load capacitors: the CH572 loads its 32 MHz crystal with
    internal tunable capacitors (CL 12 pF for the XRCGB32M000F1S1DR0).
    Its footprint is a corrected one: an earlier board put XI and XO on
    the package's joined NC pads.
- The USB-C port (J1, CC resistors R1/R2, bootloader strap JP1/R3) and its
  regulator U2 went when the board moved to a battery; see Power and
  Flashing below.
- CH572D pin numbers are from datasheet CH572DS1 Table 1-1;
  `tests/test_board.py` holds them and every net of the board.
- The project's CH572D symbol has PA4, PA5, PA6, PA8 and PA9 at 3.81 mm
  instead of 2.54 mm, as PA10/PA11 already were: they carry no-connect
  flags, and at 2.54 mm the flag's cross lands on the two-digit pin number.
- ERC: +3V3 and GND carry PWR_FLAGs, since the cell connector and D3 have
  passive pins, so `power_pin_not_driven` stays at its default (error) and
  passes.

## Power

- A 3.6 V Li-SOCl2 primary cell on J1 feeds the CH572D through D3, with
  no regulator. The chip's own regulator (LDO5V) is no use here: it takes
  4.5 to 5.3 V on V5. In single-supply mode VDD33 and VIO33 take 2.0 to
  3.6 V, absolute maximum 4.0 V (Tables 19-1, 19-2), and internal LDOs
  make the core supplies. The `+3V3` net is that unregulated rail.
- D3 (BAT54WS Schottky) keeps every current out of the cell: a primary
  lithium cell must never be charged, and the WCH-LinkE's 3.3 V on J4 or a
  cell plugged in reversed would otherwise do it. Its drop also keeps a
  fresh cell (about 3.67 V open circuit) under the 3.6 V operating limit:
  at most 0.24 V at 0.1 mA and 0.4 V at 10 mA (BAT54 datasheet). Chosen
  over 1 A Schottkys with lower drop (B5819W and the like) for its lower
  reverse leakage, which is charge current into a depleted cell while J4
  powers the board.
- C1 (100 uF, X5R ceramic, 1206) supplies the radio bursts. Bobbin cells
  (ER14250, ER14505) have a high internal resistance and a passivation
  layer that delays their voltage under a sudden load. Ceramic, because
  tantalum and electrolytic parts may leak microamps (tantalum: up to
  0.01 CV, 6 uA for 100 uF at 6.3 V), more than the chip's whole sleep
  current (0.3 to 1.2 uA, chapter 5 power modes). A 6.3 V 1206 part keeps
  only about half its rated capacitance at 3.6 V DC bias; check the
  chosen part's bias curve, or use a 10 V part.
- J1 is a 2-pin JST PH (2.0 mm) horizontal header for a wire-lead cell,
  which leaves the cell size open (1/2 AA, AA). JST PH polarity is not
  standard among cell vendors: check + is on pin 1 before plugging in;
  D3 blocks a reversed cell.
- End of life: the chip resets below Vlvr (1.5 to 1.9 V); the battery
  monitor (`R8_BAT_DET_CFG`, thresholds 1.8 to 2.4 V, about 1 uA when
  enabled) can warn first.

## Flashing

- PA0 and PA1 are the two-wire debug pins SWDIO and SWCLK, enabled after
  every power-on and reset (Table 1-1, note 3). J4 (1x04, 2.54 mm) brings
  them out for a WCH-LinkE: 1 +3V3, 2 SWDIO, 3 SWCLK, 4 GND.
- J4 is the only way into the chip: firmware must leave the debug
  interface enabled, since disabling it frees PA0/PA1 as GPIO but locks
  out the programmer. (WCH's BLE examples disable it at start; this
  firmware does not.)
- A chip in Sleep does not answer the probe, and the firmware sleeps
  between polls. It stays awake for 5 s after every reset (D2 lit), so:
  power-cycle the board, then `mise run fw:flash` within those 5 s. If
  that window is missed with the WCH-LinkE powering the board (cell
  unplugged), `wlink --chip CH572 erase --method power-off` cuts the
  probe's 3.3 V and erases on the way back up. wlink cannot detect the
  CH57x family, hence `--chip`.

## Touch wire

- The CH572 has no touch-key peripheral and no ADC. Its analog input is
  one comparator (datasheet chapter 14): positive input PA3 (CMP_P0) or
  PA7 (CMP_P1), negative input PA2 or an internal reference of 50 to
  800 mV in 50 mV steps (`R32_CMP_CTRL.RB_CMP_NREF_LEVEL`), and
  `RB_CMP_CAP` routes its output to the timer capture input. KEYSCAN
  (chapter 15) scans mechanical keys with internal pulls and cannot sense
  capacitance.
- So touch is RC timing: PA10 drives the wire high through R7 (1M), and the
  timer captures when the wire's voltage crosses the comparator reference
  on PA7. A finger adds capacitance and delays the crossing. PA7, not PA3:
  PA3 drives LED D1. PA10 (pin 17) sits next to PA7 (pin 16) on the
  package, which keeps the two traces short.
- Timing: t = R7 * C * ln(3.3 / (3.3 - Vref)). At Vref = 0.8 V that is
  0.28 * R7 * C: about 2.8 us for 10 pF of pad, wire and pin, and each pF a
  finger adds is 0.28 us, 28 counts of a 100 MHz timer (the CH572's top
  system clock). A long wire raises the base capacitance and the time;
  firmware tracks the baseline and looks for the step.
- Discharge between measurements: PA10 low empties the wire through R7
  (5 * R7 * C, 50 us at 10 pF). Firmware may instead drive PA7 low through
  R6, which is 100 times faster, but CMP_P1 is listed as analog input only
  (Table 7-6), so check on the bench that the pin can switch back.
- R6 (10k) sits between the wire and PA7 and R7 between the wire and PA10,
  so ESD on the touch wire reaches both pins through a resistor. R6 delays
  PA7 behind the wire by R6 times PA7's input capacitance (tens of ns),
  under 2 % of a 2.8 us measurement.
- PA7 is also the optional external reset input: setting both
  `CFG_RESET_EN` and `CFG_RST_PIN_EN` in the non-volatile config moves
  reset onto PA7 and breaks the touch input. Leave `CFG_RESET_EN` at 0.
- J2 is a plated hole for wires up to 0.5 mm^2 (0.9 mm drill).

## Antenna

- The CH572 has its antenna matching network on chip; the datasheet
  (pin table and 18.1) recommends connecting ANT straight to the antenna.
  C5, C6 (DNP) and R8 (0R) are a pi-match placeholder: fitted as drawn it
  is a straight connection, and values go in only if measuring the built
  board with the final antenna and enclosure asks for them.
- The antenna is J3, a U.FL connector for an external 2.4 GHz antenna.

## Firmware

Rust in `firmware/`, started from
[ch572-bridge-rs](https://github.com/tmpk13/ch572-bridge-rs) (startup,
clock setup, the WCH BLE bindings). Its BLE build never ran on hardware,
and this firmware has not either: everything below is from WCH's EVT
(github.com/openwch/ch570, commit 1e34c94) and the datasheet, checked by
host tests and the disassembly only. The app-facing protocol is
[docs/BLE.md](docs/BLE.md).

- BLE needs WCH's closed-source stack (`libCH572BLE_PERI.a`): the
  datasheet documents no radio registers. `mise run fw:wch` downloads it
  and `libISP572.a` (for the MAC read) from the pinned commit, SHA-256
  checked; they are not committed.
- Sleep follows WCH's HAL_SLEEP path (EVT `BLE/HAL/SLEEP.c`,
  `CH57x_pwr.c` LowPower_Sleep): TMOS calls the idle callback with the RTC
  time of its next event, the firmware sets the RTC alarm 1.6 ms earlier
  (crystal restart) and enters Sleep keeping RAM, core and radio
  configuration, about 1.2 uA by datasheet table 5-3. The wake-up arithmetic
  across the RTC's daily wrap is in `sleep.rs`, tested.
- Sleep powers flash down, so whatever runs around it lives in RAM: the
  vector table (the RTC interrupt is taken on the way out of the WFI), the
  RTC handler, the sleep entry, the main loop, TMOS_SystemProcess and the
  library's `.highcode_lp` section (tmos_idle and the radio re-init).
  WCH's link script leaves `.highcode_lp` unplaced; RAM is the safe side.
  The disassembly shows the firmware's RAM functions call nothing in
  flash. RAM: 5.2 KB of code, 4.9 KB of .bss (3.5 KB of it the BLE heap),
  2.1 KB of stack; the diagnostics characteristic reports how much stack
  stayed unused.
- Unique ID: the factory MAC address in the info area (0x3F018). On this
  chip WCH's GET_UNIQUE_ID is that MAC plus a 16-bit sum of it, which is
  the datasheet's 8-byte unique ID (section 4.5). It is the BLE public
  address, the device ID characteristic and the `PTT-xxxxxx` name. A blank
  MAC (all 0 or all FF) stops the firmware with D2 blinking once per
  cycle; uniqueness across chips rests on WCH's factory programming.
- Touch measurement (`hw::measure_touch`): the timer runs in capture mode,
  edge to edge, with RB_CMP_CAP feeding it the comparator; counting
  starts one store before PA10 rises, and the first FIFO entry is taken as
  the charge time. That the first entry counts from the start of counting
  is inferred from the datasheet's timeout behavior (8.3.3), not stated.
  If it is wrong, charges time out while the comparator reads high, which
  sets the capture-fault flag in the diagnostics; the fallback is polling
  the comparator's live output (RB_APR_OUT_CMP) against SysTick at about
  50 ns resolution. The comparator's settling time after enable (10 us
  allowed) is a guess too.
- Discharge between samples lasts 25 charge times (7 RC), so each sample
  starts from the same residual voltage whatever the wire's capacitance.
- Detector constants in `touch.rs` (threshold 1/16 of the baseline, at
  least 30 counts; two polls to press and to release; 2 minute stuck
  limit) are starting points: tune them on the built board with the
  diagnostics characteristic in nRF Connect.
- Crystal load RB_XT32M_C_LOAD = 110b (18 pF), as WCH's evaluation board
  without external load capacitors uses; the radio carrier depends on it,
  so measure the offset on the built board.
- Power, untested: the touch poll and the advertising event are separate
  wakes 200 ms apart each, and each wake pays the crystal restart. Folding
  the poll into the advertising event would halve the wakes when idle.
  Fsys stays at WCH's BLE example clock (PLL 100 MHz); a lower clock
  would cut the awake current but is untried with the library. D1 is lit
  for the length of every press.
- No pairing or bonding: any phone in range can read the switch. Bonds
  would need the library's SNV storage in flash (WCH puts it at 0x3B000,
  which link.x would have to leave out of FLASH).

## Enclosure

### params.toml is the source, the FCStd is generated

`enclosure/build/ptt-box.FCStd` is rebuilt from `enclosure/params.toml` by
`mise run box:build` and is not committed. Inside it, every feature reads the
"Params" spreadsheet through expressions, so changing a value there and
recomputing reshapes the model in the GUI. That is for trying values; the
next build overwrites it, so settled values go back into params.toml.

All values in params.toml are placeholders until the board outline, its
mounting holes and the touch wire layout are final. The cable opening is
sized for a USB-C plug, which the board no longer has; the battery and the
SWD header J4 need room instead.

### Part primitives and expressions, no sketches

Every solid is a Part workbench primitive whose sizes and placement are
spreadsheet expressions. Rounded rectangles are a Plane shrunk by the corner
radius on each side, grown back by an Offset2D with arc joins, then
extruded. Nothing references a face or edge by name and there is no sketch
solver, so value changes cannot break topology references.

Offset2D fails at a radius of zero, so the derived inner radii clamp to
`r_min` (0.1 mm). When `corner_r` is at most `wall`, the cavity corners come
out nearly sharp; the wall is thicker there, never thinner.

### Fit rules the build enforces

`box.check` rejects values that build a solid but give a box that does not
work: the lid lip reaching down to the PCB top (`lip_h` must stay below
`above_pcb`), the cable opening running into the floor or behind the lip,
the touch dimple running past the cavity outline, a pilot hole as wide as
its standoff, and `touch_skin` not thinner than `lid_t`. The lid is held only
by the friction fit of its lip (`fit_gap` per side); there are no screws or
snaps yet.

### Touch wire against the lid

Capacitive sensitivity falls off sharply with any air gap between the
electrode and the cover, so the end of the J2 touch wire (or a foil pad on
it) belongs flat against the lid underside, under the finger dimple. The
dimple thins the lid from outside for the same reason: thinning from inside
would leave a gap. Nothing holds the wire there yet; a clip or channel
under the dimple waits for the wire and pad choice. The box has no opening
for the external antenna on J3 either.

### Tests run in FreeCAD's Python

The enclosure scripts need FreeCAD's own modules, so they run under
`freecadcmd` with the Python bundled in FreeCAD, not the pixi environment,
and use `unittest` because that Python has no pytest. The geometry tests
derive expected sizes from params.toml, so they hold for the final values.

### freecadcmd script quirks

- A script file runs as a module named after the file, never as
  `__main__`: an `if __name__ == "__main__":` block silently does nothing.
- The script's folder is not on `sys.path`; `build.py` and `test_box.py`
  insert it.
- Buffered `sys.stdout` is dropped when the script calls `sys.exit`, so
  flush first. The exit code does reach the shell.
- `freecadcmd -c CODE` with further arguments opens the interactive
  console instead of exiting, so scripts take settings from environment
  variables (`BOX_PARAMS`, `BOX_OUT`).
- OpenCASCADE's STEP writer and the document and mesh savers print
  statistics and percent counters to file descriptor 1, partly through
  libc's stdio buffer. `box.quiet_stdout` points fd 1 at /dev/null and calls
  `fflush(NULL)` before restoring it; without the flush the counters appear
  at exit.

### FreeCAD location

mise does not install FreeCAD. The `box:*` tasks call `$FREECADCMD`
(headless) and `$FREECAD` (GUI), defaulting to `freecadcmd` and `freecad` on
PATH. For an AppImage, set both in `mise.local.toml` (not committed); the
AppImage runs a bundled tool when given its name as the first argument:

```toml
[env]
FREECAD = "{{env.HOME}}/apps/FreeCAD.AppImage"
FREECADCMD = "{{env.HOME}}/apps/FreeCAD.AppImage freecadcmd"
```
