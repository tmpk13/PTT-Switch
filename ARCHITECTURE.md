# Architecture

## Board

```mermaid
flowchart LR
    cell[J1 Li-SOCl2 cell 3.6 V]
    d3[D3 BAT54WS]
    c1[C1 100 uF]
    swd[J4 SWD header]
    subgraph mcu[U1 CH572D]
        cmp[comparator<br/>CMP_P1 = PA7]
        tmr[timer capture]
        gpio[PA10]
        ble[BLE radio<br/>ANT]
        dbg[debug interface<br/>PA0/PA1]
        leds[PA2, PA3]
    end
    y1[Y1 32 MHz]
    wire[J2 touch wire]
    r7[R7 1M]
    r6[R6 10k]
    match[C5, R8, C6 pi-match]
    ant[J3 U.FL]
    d[D1, D2 LEDs]

    cell -- VBAT --> d3 -- +3V3 --> mcu
    c1 --- d3
    swd -- +3V3 --> mcu
    swd -- SWDIO/SWCLK --> dbg
    y1 --> mcu
    gpio --> r7 --> wire
    wire --> r6 --> cmp --> tmr
    ble --> match --> ant
    leds --> d
```

## One touch measurement, in order

Durations show order only, not time; the charge step is the measured one
(about 2.8 us at 10 pF, see [NOTES.md](NOTES.md)).

```mermaid
gantt
    dateFormat X
    axisFormat %s
    section PA10
    Low: discharge the wire through R7 :d1, 0, 5
    High: charge the wire through R7   :d2, after d1, 3
    section Comparator on PA7
    Below the reference                :c1, 0, 6
    Crosses the reference, output high :c2, after c1, 2
    section Timer
    Start at the PA10 rising edge      :t1, after d1, 1
    Capture on the comparator output   :t2, after c1, 1
```

## Enclosure build

```mermaid
flowchart LR
    params["enclosure/params.toml<br/>input dimensions"]
    kicad["ptt-switch/<br/>KiCad PCB"]
    script["enclosure/box.py<br/>run by build.py under freecadcmd"]
    sheet["Params spreadsheet<br/>inputs + derived formulas"]
    base["Base<br/>shell, standoffs, pilot holes, USB-C opening"]
    lid["Lid<br/>plate, lip, touch dimple"]
    pcb["Pcb<br/>stand-in for fit checks"]
    out["enclosure/build/<br/>ptt-box.FCStd, STEP and STL per part"]

    kicad -. "board size and holes, copied by hand" .-> params
    params --> script
    script --> sheet
    sheet -- expressions --> base
    sheet -- expressions --> lid
    sheet -- expressions --> pcb
    base --> out
    lid --> out
```

## Enclosure section, bottom to top

Z = 0 is the underside of the floor; the PCB is centered on the Z axis.

```mermaid
flowchart BT
    floor["floor: 0 .. floor_t"]
    standoff["standoffs: floor_t .. pcb_z<br/>pcb_z = floor_t + standoff_h"]
    board["PCB: pcb_z .. pcb_top"]
    air["above_pcb clearance: pcb_top .. base_h"]
    lip["lid lip: base_h - lip_h .. base_h, inside the walls"]
    plate["lid plate: base_h .. total_h"]
    floor --> standoff --> board --> air --> plate
    lip -. "hangs into" .-> air
```
