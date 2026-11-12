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
    (datasheet power-pin requirement). U2 (AP2112K-3.3) makes +3V3 from
    VBUS.
  - Y1 has no load capacitors: the CH572 loads its 32 MHz crystal with
    internal tunable capacitors (CL 12 pF for the XRCGB32M000F1S1DR0).
    Its footprint is a corrected one: an earlier board put XI and XO on
    the package's joined NC pads.
  - JP1 shorted while plugging in USB pulls D+ to 3.3 V through R3: the
    CH572D starts its ROM bootloader, the way to flash it over USB. Leave
    it open otherwise.
  - The USB pair is named `USB_D+`/`USB_D-` so the PCB diff-pair router
    pairs it.
- CH572D pin numbers are from datasheet CH572DS1 Table 1-1;
  `tests/test_board.py` holds them and every net of the board.
- The project's CH572D symbol has PA4, PA5, PA6, PA8 and PA9 at 3.81 mm
  instead of 2.54 mm, as PA10/PA11 already were: they carry no-connect
  flags, and at 2.54 mm the flag's cross lands on the two-digit pin number.
- ERC: VBUS and GND carry PWR_FLAGs and U2's output drives +3V3, so
  `power_pin_not_driven` stays at its default (error) and passes.

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
