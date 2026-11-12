"""The CH572D symbol and footprint against the datasheet, and the whole
schematic netlist, as exported by KiCad, against the intended circuit.

Pin numbers are from the CH572 datasheet (CH572DS1) Table 1-1, CH572D column,
cross-checked against its pinout drawing; the exposed pad (datasheet pin 0)
is pad 21.
"""

import re
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
PROJ = ROOT / "ptt-switch"

TABLE_1_1 = {
    "1": "PA3", "2": "XO", "3": "XI", "4": "NC", "5": "ANT", "6": "VDD33",
    "7": "VIO33", "8": "V5", "9": "PA9", "10": "PA8", "11": "PA0_UDM",
    "12": "PA1_UDP", "13": "PA4", "14": "PA5", "15": "PA6", "16": "PA7",
    "17": "PA10", "18": "PA11", "19": "NC", "20": "PA2", "21": "GND",
}

# Every net of the board as a set of REF.PIN, by function.
NETS = {
    "+3V3": {"U2.5", "C2.1", "C3.1", "C4.1", "U1.6", "U1.7", "U1.8", "R3.1", "D1.2", "D2.2"},
    "GND": {"U1.21", "U2.2", "J1.A1", "J1.A12", "J1.B1", "J1.B12", "J1.SH", "R1.2", "R2.2",
            "Y1.2", "C1.2", "C2.2", "C3.2", "C4.2", "C5.2", "C6.2", "J3.2"},
    "VBUS": {"J1.A4", "J1.A9", "J1.B4", "J1.B9", "C1.1", "U2.1", "U2.3"},
    # USB-C data pair, CC sink resistors, CH572D ROM bootloader strap
    "USB_D+": {"J1.A6", "J1.B6", "U1.12", "JP1.2"},
    "USB_D-": {"J1.A7", "J1.B7", "U1.11"},
    "CC1": {"J1.A5", "R1.1"},
    "CC2": {"J1.B5", "R2.1"},
    "BOOT_STRAP": {"R3.2", "JP1.1"},
    # 32 MHz crystal, loaded by the CH572's internal capacitors
    "XO": {"U1.2", "Y1.1"},
    "XI": {"U1.3", "Y1.3"},
    # LEDs from +3V3, pulled low by PA3 and PA2
    "LED1": {"U1.1", "R4.1"},
    "LED1_K": {"R4.2", "D1.1"},
    "LED2": {"U1.20", "R5.1"},
    "LED2_K": {"R5.2", "D2.1"},
    # touch wire: PA10 charges it through R7, PA7 (CMP_P1) senses through R6
    "TOUCH": {"J2.1", "R6.1", "R7.1"},
    "TOUCH_SENSE": {"U1.16", "R6.2"},
    "TOUCH_DRIVE": {"U1.17", "R7.2"},
    # ANT through the pi-match (C5 DNP, R8 0R, C6 DNP) to the U.FL
    "ANT": {"U1.5", "C5.1", "R8.2"},
    "ANT_FEED": {"R8.1", "C6.1", "J3.1"},
}
UNCONNECTED = {"U1.4", "U1.19", "U1.9", "U1.10", "U1.13", "U1.14", "U1.15", "U1.18",
               "U2.4", "J1.A8", "J1.B8"}
# nets the PCB needs by name (net classes, keep-outs)
NAMED = {"TOUCH", "ANT", "USB_D+", "USB_D-", "+3V3", "GND", "VBUS"}


def parse(text):
    """Nested lists from an s-expression; quoted strings lose their quotes."""
    tokens = re.findall(r'\(|\)|"(?:[^"\\]|\\.)*"|[^\s()]+', text)
    stack = [[]]
    for tok in tokens:
        if tok == "(":
            stack.append([])
        elif tok == ")":
            done = stack.pop()
            stack[-1].append(done)
        else:
            stack[-1].append(tok[1:-1] if tok.startswith('"') else tok)
    return stack[0][0]


def children(node, key):
    return [c for c in node if isinstance(c, list) and c and c[0] == key]


def value(node, key):
    return children(node, key)[0][1]


def test_symbol_pins_match_table_1_1():
    lib = parse((PROJ / "lib" / "CH572D.kicad_sym").read_text())
    sym = children(lib, "symbol")[0]
    pins = {}
    for unit in children(sym, "symbol"):
        for pin in children(unit, "pin"):
            pins[value(pin, "number")] = value(pin, "name")
    assert pins == TABLE_1_1


def test_footprint_has_every_pad():
    fp = parse((PROJ / "libraries" / "ptt-switch.pretty" / "CH572D.kicad_mod").read_text())
    pads = {p[1] for p in children(fp, "pad") if p[1]}
    assert pads == set(TABLE_1_1)


@pytest.fixture(scope="module")
def netlist(tmp_path_factory):
    cli = shutil.which("kicad-cli") or ""
    if not cli:
        pytest.skip("kicad-cli not installed")
    # export from a copy: kicad-cli may rewrite the sheets it reads
    work = tmp_path_factory.mktemp("proj")
    shutil.copytree(PROJ, work / "p")
    out = work / "board.net"
    subprocess.run([cli, "sch", "export", "netlist", "--format", "kicadsexpr", "-o", str(out),
                    str(work / "p" / "ptt-switch.kicad_sch")], check=True, capture_output=True)
    nets = {}
    for net in children(children(parse(out.read_text()), "nets")[0], "net"):
        nets[value(net, "name")] = frozenset(
            f'{value(n, "ref")}.{value(n, "pin")}' for n in children(net, "node"))
    return nets


def test_netlist_is_the_intended_circuit(netlist):
    want = {frozenset(p) for p in NETS.values()} | {frozenset({p}) for p in UNCONNECTED}
    got = set(netlist.values())
    assert got - want == set(), "nets drawn but not intended"
    assert want - got == set(), "nets intended but not drawn"


def test_named_nets(netlist):
    # a local label shows up as /NAME, a power symbol as NAME
    names = {name.rsplit("/", 1)[-1] for name in netlist}
    assert NAMED <= names
    assert netlist["/TOUCH"] == NETS["TOUCH"]
    assert netlist["/ANT"] == NETS["ANT"]
