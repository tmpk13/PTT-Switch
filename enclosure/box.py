"""Parametric two-part enclosure for the PTT switch: a base tray that holds
the PCB on four screw standoffs, and a lid with a locating lip and a finger
dimple over the touch electrode.

Every dimension lives in the "Params" spreadsheet of the generated FreeCAD
document. Each feature reads it through expressions, so changing a value in
the spreadsheet and recomputing reshapes the whole box. Input values come
from params.toml; the derived values below are spreadsheet formulas.

Coordinates: X along the PCB length, Y along its width, Z up. The PCB and the
cavity are centered on the Z axis; Z = 0 is the underside of the base floor.
The lid is placed where it sits on the closed box.

Runs inside FreeCAD's Python (freecadcmd or the GUI), not a plain Python.
"""

import contextlib
import ctypes
import os
import re
import sys
import tomllib

import FreeCAD as App
import MeshPart
import Part  # noqa: F401  (loads the Part feature types)

SHEET = "Params"
DOC_NAME = "ptt_box"
STEM = "ptt-box"

# Input parameters, grouped as in params.toml: (alias, description).
INPUTS = {
    "pcb": [
        ("pcb_l", "PCB length along X"),
        ("pcb_w", "PCB width along Y"),
        ("pcb_t", "PCB thickness"),
        ("pcb_gap", "Gap from PCB edge to inner wall, each side"),
        ("above_pcb", "Clear height from PCB top to lid underside"),
    ],
    "mounting": [
        ("hole_inset", "Mounting hole center to PCB edge, in X and in Y"),
        ("hole_d", "Screw pilot hole diameter in the standoffs"),
        ("standoff_d", "Standoff outer diameter"),
        ("standoff_h", "Standoff height, floor top to PCB underside"),
    ],
    "shell": [
        ("wall", "Side wall thickness"),
        ("floor_t", "Base floor thickness"),
        ("lid_t", "Lid plate thickness"),
        ("corner_r", "Outer radius of the vertical corners"),
    ],
    "lid": [
        ("lip_h", "Height of the locating lip under the lid plate"),
        ("lip_t", "Lip wall thickness"),
        ("fit_gap", "Gap from lip to base inner wall, each side"),
    ],
    "touch": [
        ("touch_d", "Finger dimple diameter, over the touch electrode"),
        ("touch_x", "Dimple center X offset from PCB center"),
        ("touch_y", "Dimple center Y offset from PCB center"),
        ("touch_skin", "Lid thickness left under the dimple"),
    ],
    "port": [
        ("port_w", "Cable opening width (Y), in the -X end wall"),
        ("port_h", "Cable opening height (Z)"),
        ("port_y", "Opening center Y offset from PCB center"),
        ("port_z", "Opening center height above the PCB top"),
    ],
}

# Inputs that are offsets and may be zero or negative; all others must be > 0.
SIGNED = {"touch_x", "touch_y", "port_y", "port_z"}

# Derived values: (alias, spreadsheet formula, description). Formulas may
# name any input or earlier derived alias.
DERIVED = [
    ("r_min", "0.1 mm", "Smallest corner radius, used where a wall eats the outer one"),
    ("in_l", "pcb_l + 2 * pcb_gap", "Cavity length"),
    ("in_w", "pcb_w + 2 * pcb_gap", "Cavity width"),
    ("out_l", "in_l + 2 * wall", "Outer length"),
    ("out_w", "in_w + 2 * wall", "Outer width"),
    ("pcb_z", "floor_t + standoff_h", "Height of the PCB underside"),
    ("pcb_top", "pcb_z + pcb_t", "Height of the PCB top"),
    ("base_h", "pcb_top + above_pcb", "Base height, floor underside to rim"),
    ("total_h", "base_h + lid_t", "Closed box height"),
    ("r_in", "max(corner_r - wall; r_min)", "Cavity corner radius"),
    ("lip_l", "in_l - 2 * fit_gap", "Lip outer length"),
    ("lip_w", "in_w - 2 * fit_gap", "Lip outer width"),
    ("r_lip", "max(r_in - fit_gap; r_min)", "Lip outer corner radius"),
    ("r_lip_in", "max(r_lip - lip_t; r_min)", "Lip inner corner radius"),
    ("hole_x", "pcb_l / 2 - hole_inset", "Mounting hole X offset from center"),
    ("hole_y", "pcb_w / 2 - hole_inset", "Mounting hole Y offset from center"),
    ("port_zc", "pcb_top + port_z", "Cable opening center height"),
]

INPUT_ALIASES = [alias for group in INPUTS.values() for alias, _ in group]
ALIASES = set(INPUT_ALIASES) | {alias for alias, _, _ in DERIVED}

# Corner signs of the four mounting holes.
CORNERS = [(1, 1), (-1, 1), (-1, -1), (1, -1)]


class ParamError(ValueError):
    """Parameters that are missing, malformed or give an unbuildable box."""


def load_params(path):
    """Read params.toml into {alias: value in mm}, rejecting missing,
    unknown or non-numeric entries."""
    with open(path, "rb") as f:
        data = tomllib.load(f)
    values = {}
    problems = []
    for section in data.keys() - INPUTS.keys():
        problems.append(f"unknown section [{section}]")
    for section, entries in INPUTS.items():
        table = data.get(section, {})
        known = {alias for alias, _ in entries}
        for key in table.keys() - known:
            problems.append(f"unknown key [{section}] {key}")
        for alias in known:
            value = table.get(alias)
            if value is None:
                problems.append(f"missing [{section}] {alias}")
            elif isinstance(value, bool) or not isinstance(value, (int, float)):
                problems.append(f"[{section}] {alias} is not a number")
            else:
                values[alias] = float(value)
    if problems:
        raise ParamError(f"{path}: " + "; ".join(sorted(problems)))
    return values


def mm(sheet, alias):
    """Value of a spreadsheet alias in mm, as a float."""
    value = getattr(sheet, alias)
    return value.Value if hasattr(value, "Value") else float(value)


def check(sheet):
    """Problems with the values in the spreadsheet that would give a box
    that builds but does not work, as a list of messages."""
    v = {alias: mm(sheet, alias) for alias in ALIASES}
    problems = []
    for alias in INPUT_ALIASES:
        if alias not in SIGNED and v[alias] <= 0:
            problems.append(f"{alias} must be > 0")
    if v["hole_d"] >= v["standoff_d"]:
        problems.append("hole_d must be smaller than standoff_d")
    if v["hole_x"] <= 0 or v["hole_y"] <= 0:
        problems.append("hole_inset puts the mounting holes past the PCB center")
    if v["lip_h"] >= v["above_pcb"]:
        problems.append("lip_h reaches the PCB top; keep it below above_pcb")
    if v["touch_skin"] >= v["lid_t"]:
        problems.append("touch_skin must be thinner than lid_t")
    touch_r = v["touch_d"] / 2
    if (abs(v["touch_x"]) + touch_r > v["in_l"] / 2
            or abs(v["touch_y"]) + touch_r > v["in_w"] / 2):
        problems.append("touch dimple runs past the cavity outline")
    if abs(v["port_y"]) + v["port_w"] / 2 > v["in_w"] / 2:
        problems.append("cable opening runs past the cavity width")
    if v["port_zc"] - v["port_h"] / 2 < v["floor_t"]:
        problems.append("cable opening reaches into the floor")
    if v["port_zc"] + v["port_h"] / 2 > v["base_h"] - v["lip_h"]:
        problems.append("cable opening reaches the lid lip")
    return problems


def make_sheet(doc, params):
    """Spreadsheet with the inputs, then the derived formulas, one per row:
    column A the alias name, B the aliased value, C the description."""
    sheet = doc.addObject("Spreadsheet::Sheet", SHEET)
    sheet.set("A1", "Parameter")
    sheet.set("B1", "Value")
    sheet.set("C1", "Description")
    row = 2
    for section, entries in INPUTS.items():
        sheet.set(f"A{row}", f"[{section}]")
        row += 1
        for alias, description in entries:
            sheet.set(f"A{row}", alias)
            sheet.set(f"B{row}", f"{params[alias]} mm")
            sheet.setAlias(f"B{row}", alias)
            sheet.set(f"C{row}", description)
            row += 1
    sheet.set(f"A{row}", "[derived: formulas, edit the inputs above]")
    row += 1
    for alias, formula, description in DERIVED:
        sheet.set(f"A{row}", alias)
        sheet.set(f"B{row}", f"={formula}")
        sheet.setAlias(f"B{row}", alias)
        sheet.set(f"C{row}", description)
        row += 1
    sheet.setColumnWidth("A", 160)
    sheet.setColumnWidth("C", 380)
    return sheet


def expr(text):
    """Expression text with every parameter alias qualified by the
    spreadsheet name, so "out_l / 2" becomes "Params.out_l / 2"."""
    return re.sub(
        r"\b[a-z_][a-z0-9_]*\b",
        lambda m: f"{SHEET}.{m.group()}" if m.group() in ALIASES else m.group(),
        text,
    )


def bind(obj, prop, text):
    obj.setExpression(prop, expr(text))


def place(obj, x, y, z):
    bind(obj, ".Placement.Base.x", x)
    bind(obj, ".Placement.Base.y", y)
    bind(obj, ".Placement.Base.z", z)


def hide(*objs):
    for obj in objs:
        obj.Visibility = False


def slab(doc, name, l, w, h, r, z, cx="0 mm", cy="0 mm"):
    """Prism with a rounded-rectangle footprint l x w centered on (cx, cy),
    vertical corner radius r, from height z up by h. Built as a face shrunk
    by r on each side, grown back by a 2D offset with arc joins, then
    extruded, so every size stays an expression."""
    face = doc.addObject("Part::Plane", name + "Face")
    bind(face, "Length", f"({l}) - 2 * ({r})")
    bind(face, "Width", f"({w}) - 2 * ({r})")
    place(face, f"({cx}) - ({l}) / 2 + ({r})", f"({cy}) - ({w}) / 2 + ({r})", z)
    rounded = doc.addObject("Part::Offset2D", name + "Round")
    rounded.Source = face
    rounded.Join = "Arc"
    bind(rounded, "Value", r)
    prism = doc.addObject("Part::Extrusion", name)
    prism.Base = rounded
    prism.DirMode = "Custom"
    prism.Dir = App.Vector(0, 0, 1)
    prism.Solid = True
    bind(prism, "LengthFwd", h)
    hide(face, rounded, prism)
    return prism


def cylinder(doc, name, d, h, x, y, z):
    """Vertical cylinder of diameter d with its bottom center at (x, y, z)."""
    cyl = doc.addObject("Part::Cylinder", name)
    bind(cyl, "Radius", f"({d}) / 2")
    bind(cyl, "Height", h)
    place(cyl, x, y, z)
    hide(cyl)
    return cyl


def block(doc, name, lx, ly, lz, cx, cy, cz):
    """Axis-aligned box of size lx x ly x lz centered on (cx, cy, cz)."""
    box = doc.addObject("Part::Box", name)
    bind(box, "Length", lx)
    bind(box, "Width", ly)
    bind(box, "Height", lz)
    place(box, f"({cx}) - ({lx}) / 2", f"({cy}) - ({ly}) / 2", f"({cz}) - ({lz}) / 2")
    hide(box)
    return box


def cut(doc, name, base, tool):
    obj = doc.addObject("Part::Cut", name)
    obj.Base = base
    obj.Tool = tool
    obj.Refine = True
    hide(obj)
    return obj


def fuse(doc, name, shapes):
    obj = doc.addObject("Part::MultiFuse", name)
    obj.Shapes = shapes
    obj.Refine = True
    hide(obj)
    return obj


def signed(sign, alias):
    return alias if sign > 0 else f"-{alias}"


def make_base(doc):
    """Tray: rounded shell, four standoffs with pilot holes, cable opening."""
    outer = slab(doc, "BaseOuter", "out_l", "out_w", "base_h", "corner_r", "0 mm")
    # The cavity prism runs past the rim so the cut leaves an open top.
    cavity = slab(doc, "Cavity", "in_l", "in_w", "base_h", "r_in", "floor_t")
    shell = cut(doc, "BaseShell", outer, cavity)
    standoffs = []
    pilots = []
    for i, (sx, sy) in enumerate(CORNERS, start=1):
        x, y = signed(sx, "hole_x"), signed(sy, "hole_y")
        # Standoffs start at Z = 0 so they overlap the floor fully.
        standoffs.append(cylinder(doc, f"Standoff{i}", "standoff_d", "pcb_z", x, y, "0 mm"))
        pilots.append(cylinder(doc, f"Pilot{i}", "hole_d", "standoff_h + 1 mm", x, y, "floor_t"))
    body = fuse(doc, "BaseBody", [shell] + standoffs)
    # Twice the wall thickness, centered in the wall: cuts clean through
    # without reaching far into the cavity.
    port = block(doc, "Port", "2 * wall", "port_w", "port_h",
                 "-(in_l + wall) / 2", "port_y", "port_zc")
    cuts = fuse(doc, "BaseCuts", pilots + [port])
    base = cut(doc, "Base", body, cuts)
    base.Visibility = True
    return base


def make_lid(doc):
    """Flat plate on the rim, a lip that drops inside the base walls, and a
    finger dimple that thins the plate over the touch electrode."""
    plate = slab(doc, "LidPlate", "out_l", "out_w", "lid_t", "corner_r", "base_h")
    lip_outer = slab(doc, "LipOuter", "lip_l", "lip_w", "lip_h", "r_lip", "base_h - lip_h")
    # The inner prism starts below the lip so the cut leaves an open ring;
    # its top is flush with the plate underside, which it never cuts.
    lip_inner = slab(doc, "LipInner", "lip_l - 2 * lip_t", "lip_w - 2 * lip_t",
                     "lip_h + 1 mm", "r_lip_in", "base_h - lip_h - 1 mm")
    lip = cut(doc, "Lip", lip_outer, lip_inner)
    body = fuse(doc, "LidBody", [plate, lip])
    dimple = cylinder(doc, "Dimple", "touch_d", "lid_t", "touch_x", "touch_y",
                      "base_h + touch_skin")
    lid = cut(doc, "Lid", body, dimple)
    lid.Visibility = True
    return lid


def make_pcb(doc):
    """Stand-in PCB slab for fit checks; not exported."""
    pcb = block(doc, "Pcb", "pcb_l", "pcb_w", "pcb_t", "0 mm", "0 mm", "pcb_z + pcb_t / 2")
    pcb.Visibility = True
    return pcb


def broken(doc):
    """Names of objects that failed to recompute or hold an invalid shape."""
    bad = []
    for obj in doc.Objects:
        if "Invalid" in obj.State or "Error" in obj.State:
            bad.append(obj.Name)
        elif hasattr(obj, "Shape") and not obj.Shape.isNull() and not obj.Shape.isValid():
            bad.append(obj.Name)
    return bad


def build(params, doc_name=DOC_NAME):
    """New FreeCAD document holding the spreadsheet, Base, Lid and Pcb.
    Raises ParamError, closing the document, when the parameters give a
    box that would not work or did not recompute."""
    doc = App.newDocument(doc_name)
    try:
        sheet = make_sheet(doc, params)
        doc.recompute()
        problems = check(sheet)
        if problems:
            raise ParamError("; ".join(problems))
        make_base(doc)
        make_lid(doc)
        make_pcb(doc)
        doc.recompute()
        bad = broken(doc)
        if bad:
            raise ParamError("recompute failed for " + ", ".join(bad))
    except Exception:
        App.closeDocument(doc.Name)
        raise
    return doc


def lid_print_shape(lid):
    """Lid shape turned upside down so the top face lies on Z = 0, the way
    it prints: the lip points up and needs no support."""
    shape = lid.Shape.copy()
    shape.rotate(App.Vector(0, 0, 0), App.Vector(1, 0, 0), 180)
    shape.translate(App.Vector(0, 0, -shape.BoundBox.ZMin))
    return shape


def write_stl(shape, path):
    mesh = MeshPart.meshFromShape(
        Shape=shape, LinearDeflection=0.02, AngularDeflection=0.2, Relative=False)
    mesh.write(path)


@contextlib.contextmanager
def quiet_stdout():
    """Send file descriptor 1 to /dev/null for the block. OpenCASCADE's STEP
    writer and the document and mesh savers print transfer statistics and
    percent counters straight to it, bypassing sys.stdout. Part of that
    sits in libc's stdio buffer, so it is flushed before fd 1 comes back;
    otherwise it reaches the terminal at exit."""
    sys.stdout.flush()
    saved = os.dup(1)
    devnull = os.open(os.devnull, os.O_WRONLY)
    try:
        os.dup2(devnull, 1)
        yield
    finally:
        ctypes.CDLL(None).fflush(None)
        os.dup2(saved, 1)
        os.close(devnull)
        os.close(saved)


def export(doc, out_dir):
    """Write the document, a STEP per part in assembled position, and an
    STL per part in print orientation. Returns the written paths."""
    os.makedirs(out_dir, exist_ok=True)
    base = doc.getObject("Base")
    lid = doc.getObject("Lid")
    paths = {
        "fcstd": os.path.join(out_dir, f"{STEM}.FCStd"),
        "base_step": os.path.join(out_dir, f"{STEM}-base.step"),
        "lid_step": os.path.join(out_dir, f"{STEM}-lid.step"),
        "base_stl": os.path.join(out_dir, f"{STEM}-base.stl"),
        "lid_stl": os.path.join(out_dir, f"{STEM}-lid.stl"),
    }
    with quiet_stdout():
        doc.saveAs(paths["fcstd"])
        base.Shape.exportStep(paths["base_step"])
        lid.Shape.exportStep(paths["lid_step"])
        write_stl(base.Shape, paths["base_stl"])
        write_stl(lid_print_shape(lid), paths["lid_stl"])
    return paths


def main():
    """Build from params.toml next to this file into ./build. The
    environment variables BOX_PARAMS and BOX_OUT override the two paths."""
    here = os.path.dirname(os.path.abspath(__file__))
    params_path = os.environ.get("BOX_PARAMS", os.path.join(here, "params.toml"))
    out_dir = os.environ.get("BOX_OUT", os.path.join(here, "build"))
    doc = build(load_params(params_path))
    paths = export(doc, out_dir)
    sheet = doc.getObject(SHEET)
    print("outer %.2f x %.2f x %.2f mm" % (
        mm(sheet, "out_l"), mm(sheet, "out_w"), mm(sheet, "total_h")))
    for path in paths.values():
        print("wrote", os.path.relpath(path))
