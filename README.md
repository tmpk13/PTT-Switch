# ptt-switch

A push-to-talk switch: a wire on the board is a capacitive touch electrode,
and a WCH CH572D (RISC-V, BLE 5) senses it and reports over Bluetooth Low
Energy. KiCad 10 project in `ptt-switch/`, powered by a 3.6 V Li-SOCl2
cell and flashed through an SWD header with a WCH-LinkE;
`enclosure/` builds a parametric FreeCAD box for it from
`enclosure/params.toml`. `firmware/` is the CH572D's Rust firmware: it
sleeps between touch polls and reports the wire's state to a phone app
as described in [docs/BLE.md](docs/BLE.md). `android/` is that app: for
now it finds a board, connects and shows whether the wire is touched.

| Task | Command |
|------|---------|
| Tests: CH572D pinout, netlist against the intended circuit, enclosure geometry, firmware logic, app protocol | `mise run test` |
| Firmware: build, then flash through a WCH-LinkE on J4 (within 5 s of power-up) | `mise run fw:flash` |
| Firmware: build only | `mise run fw:build` |
| ERC (report in `build/erc.rpt`) | `mise run erc` |
| Enclosure: FCStd, STEP and STL per part into `enclosure/build/` | `mise run box:build` |
| Enclosure: build, then open in the FreeCAD GUI | `mise run box:open` |
| App: build, install on the phone attached over adb, start | `mise run app:install` |

FreeCAD is not installed by mise; see
[NOTES.md](NOTES.md#freecad-location) where it is not on PATH.

Design decisions and their reasons: [NOTES.md](NOTES.md). Block diagram and
the touch measurement sequence: [ARCHITECTURE.md](ARCHITECTURE.md).
