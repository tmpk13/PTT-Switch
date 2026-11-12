# ptt-switch

A push-to-talk switch: a wire on the board is a capacitive touch electrode,
and a WCH CH572D (RISC-V, BLE 5) senses it and reports over Bluetooth Low
Energy. KiCad 10 project in `ptt-switch/`, powered and flashed over USB-C;
`enclosure/` builds a parametric FreeCAD box for it from
`enclosure/params.toml`.

| Task | Command |
|------|---------|
| Tests: CH572D pinout, netlist against the intended circuit, enclosure geometry | `mise run test` |
| ERC (report in `build/erc.rpt`) | `mise run erc` |
| Enclosure: FCStd, STEP and STL per part into `enclosure/build/` | `mise run box:build` |
| Enclosure: build, then open in the FreeCAD GUI | `mise run box:open` |

FreeCAD is not installed by mise; see
[NOTES.md](NOTES.md#freecad-location) where it is not on PATH.

Design decisions and their reasons: [NOTES.md](NOTES.md). Block diagram and
the touch measurement sequence: [ARCHITECTURE.md](ARCHITECTURE.md).
