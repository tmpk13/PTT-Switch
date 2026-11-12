# ptt-switch

A push-to-talk switch: a wire on the board is a capacitive touch electrode,
and a WCH CH572D (RISC-V, BLE 5) senses it and reports over Bluetooth Low
Energy. KiCad 10 project in `ptt-switch/`, powered and flashed over USB-C.

| Task | Command |
|------|---------|
| Tests: CH572D pinout, netlist against the intended circuit | `mise run test` |
| ERC (report in `build/erc.rpt`) | `mise run erc` |

Design decisions and their reasons: [NOTES.md](NOTES.md). Block diagram and
the touch measurement sequence: [ARCHITECTURE.md](ARCHITECTURE.md).
