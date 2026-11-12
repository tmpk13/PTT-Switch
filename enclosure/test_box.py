"""Enclosure tests; run with freecadcmd (see build.py for why the entry
code below looks the way it does).

The geometry tests build from the real params.toml and compute expected
sizes from those values, so they keep passing when the placeholders are
replaced with the final PCB and touch sensor dimensions.
"""

import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import FreeCAD as App  # noqa: E402
import Part  # noqa: E402

import box  # noqa: E402

PARAMS = os.path.join(HERE, "params.toml")
TOL = 1e-6


def solid_count(obj):
    return len(obj.Shape.Solids)


def overlap(a, b):
    """Volume shared by two shapes."""
    return a.common(b).Volume


def probe_height(shape, x, y):
    """Total height of the material a thin vertical rod at (x, y) passes
    through, i.e. the wall thickness at that spot of a horizontal plate."""
    rod = Part.makeCylinder(0.05, 1000, App.Vector(x, y, -500))
    return shape.common(rod).BoundBox.ZLength


class GeometryTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.p = box.load_params(PARAMS)
        cls.doc = box.build(cls.p, "test_geometry")
        cls.base = cls.doc.getObject("Base")
        cls.lid = cls.doc.getObject("Lid")
        cls.pcb = cls.doc.getObject("Pcb")

    @classmethod
    def tearDownClass(cls):
        App.closeDocument(cls.doc.Name)

    def base_h(self):
        p = self.p
        return p["floor_t"] + p["standoff_h"] + p["pcb_t"] + p["above_pcb"]

    def test_each_part_is_one_valid_solid(self):
        for part in (self.base, self.lid):
            self.assertTrue(part.Shape.isValid(), part.Name)
            self.assertEqual(solid_count(part), 1, part.Name)

    def test_outer_size_follows_pcb_gap_and_wall(self):
        p = self.p
        out_l = p["pcb_l"] + 2 * p["pcb_gap"] + 2 * p["wall"]
        out_w = p["pcb_w"] + 2 * p["pcb_gap"] + 2 * p["wall"]
        bb = self.base.Shape.BoundBox
        self.assertAlmostEqual(bb.XLength, out_l)
        self.assertAlmostEqual(bb.YLength, out_w)
        self.assertAlmostEqual(bb.ZMin, 0)
        self.assertAlmostEqual(bb.ZMax, self.base_h())
        lid = self.lid.Shape.BoundBox
        self.assertAlmostEqual(lid.XLength, out_l)
        self.assertAlmostEqual(lid.YLength, out_w)
        self.assertAlmostEqual(lid.ZMax, self.base_h() + p["lid_t"])
        self.assertAlmostEqual(lid.ZMin, self.base_h() - p["lip_h"])

    def test_lid_and_base_do_not_collide(self):
        self.assertLess(overlap(self.base.Shape, self.lid.Shape), TOL)

    def test_pcb_rests_on_standoffs_clear_of_lid(self):
        self.assertLess(overlap(self.base.Shape, self.pcb.Shape), TOL)
        self.assertLess(overlap(self.lid.Shape, self.pcb.Shape), TOL)
        # Each standoff top touches the PCB underside: the base reaches up to
        # just below the PCB at every hole position, beside the pilot hole.
        p = self.p
        hx = p["pcb_l"] / 2 - p["hole_inset"]
        hy = p["pcb_w"] / 2 - p["hole_inset"]
        ring = (p["hole_d"] + p["standoff_d"]) / 4
        z = p["floor_t"] + p["standoff_h"] - 0.01
        for sx, sy in box.CORNERS:
            spot = App.Vector(sx * hx + ring, sy * hy, z)
            self.assertTrue(self.base.Shape.isInside(spot, TOL, True), (sx, sy))

    def test_pilot_holes_are_open(self):
        p = self.p
        hx = p["pcb_l"] / 2 - p["hole_inset"]
        hy = p["pcb_w"] / 2 - p["hole_inset"]
        z = p["floor_t"] + p["standoff_h"] / 2
        for sx, sy in box.CORNERS:
            spot = App.Vector(sx * hx, sy * hy, z)
            self.assertFalse(self.base.Shape.isInside(spot, TOL, True), (sx, sy))

    def test_lid_is_thin_only_under_the_dimple(self):
        p = self.p
        lid = self.lid.Shape
        self.assertAlmostEqual(probe_height(lid, p["touch_x"], p["touch_y"]), p["touch_skin"])
        # Next to the dimple, still over the cavity, the plate is full depth.
        x = p["touch_x"] + p["touch_d"] / 2 + 0.5
        lip_inner_x = p["pcb_l"] / 2 + p["pcb_gap"] - p["fit_gap"] - p["lip_t"]
        if x < lip_inner_x:
            self.assertAlmostEqual(probe_height(lid, x, p["touch_y"]), p["lid_t"])

    def test_cable_opening_goes_through_the_end_wall(self):
        p = self.p
        z = p["floor_t"] + p["standoff_h"] + p["pcb_t"] + p["port_z"]
        x_wall = -(p["pcb_l"] / 2 + p["pcb_gap"] + p["wall"] / 2)
        # A thin rod from outside the box to the cavity center line, through
        # the middle of the opening, touches no material.
        x_out = x_wall - 2 * p["wall"]
        rod = Part.makeCylinder(0.05, -x_out, App.Vector(x_out, p["port_y"], z), App.Vector(1, 0, 0))
        self.assertLess(overlap(self.base.Shape, rod), TOL)
        # The same height beside the opening is solid wall.
        y_beside = p["port_y"] + p["port_w"] / 2 + 0.5
        self.assertTrue(self.base.Shape.isInside(App.Vector(x_wall, y_beside, z), TOL, True))

    def test_lid_print_shape_lies_on_its_top_face(self):
        shape = box.lid_print_shape(self.lid)
        bb = shape.BoundBox
        self.assertAlmostEqual(bb.ZMin, 0)
        self.assertAlmostEqual(bb.ZLength, self.p["lid_t"] + self.p["lip_h"])
        self.assertAlmostEqual(shape.Volume, self.lid.Shape.Volume, places=6)


class SpreadsheetTest(unittest.TestCase):
    """Edits in the spreadsheet must reshape the parts after a recompute,
    which is what makes the FCStd parametric in the FreeCAD GUI."""

    def test_longer_pcb_grows_base_and_lid(self):
        p = box.load_params(PARAMS)
        doc = box.build(p, "test_sheet")
        try:
            base, lid = doc.getObject("Base"), doc.getObject("Lid")
            before = (base.Shape.BoundBox.XLength, lid.Shape.BoundBox.XLength)
            sheet = doc.getObject(box.SHEET)
            sheet.set(sheet.getCellFromAlias("pcb_l"), f"{p['pcb_l'] + 10} mm")
            doc.recompute()
            self.assertEqual(box.broken(doc), [])
            self.assertAlmostEqual(base.Shape.BoundBox.XLength, before[0] + 10)
            self.assertAlmostEqual(lid.Shape.BoundBox.XLength, before[1] + 10)
        finally:
            App.closeDocument(doc.Name)


class ParamsTest(unittest.TestCase):
    def setUp(self):
        self.p = box.load_params(PARAMS)

    def write_toml(self, text):
        f = tempfile.NamedTemporaryFile("w", suffix=".toml", delete=False)
        f.write(text)
        f.close()
        self.addCleanup(os.unlink, f.name)
        return f.name

    def assert_rejected(self, changes, fragment):
        with self.assertRaises(box.ParamError) as caught:
            box.build(dict(self.p, **changes), "test_rejected")
        self.assertIn(fragment, str(caught.exception))
        self.assertNotIn("test_rejected", App.listDocuments())

    def test_params_file_has_every_input(self):
        self.assertEqual(sorted(self.p), sorted(box.INPUT_ALIASES))

    def test_missing_and_unknown_keys_are_rejected(self):
        path = self.write_toml("[pcb]\npcb_l = 40\nbogus = 1\n[extra]\n")
        with self.assertRaises(box.ParamError) as caught:
            box.load_params(path)
        message = str(caught.exception)
        for fragment in ("missing [pcb] pcb_w", "unknown key [pcb] bogus", "unknown section [extra]"):
            self.assertIn(fragment, message)

    def test_non_number_is_rejected(self):
        with open(PARAMS) as f:
            text = f.read().replace("pcb_t = 1.6", 'pcb_t = "1.6"')
        with self.assertRaises(box.ParamError) as caught:
            box.load_params(self.write_toml(text))
        self.assertIn("pcb_t is not a number", str(caught.exception))

    def test_non_positive_size_is_rejected(self):
        self.assert_rejected({"wall": 0.0}, "wall must be > 0")

    def test_lip_reaching_pcb_is_rejected(self):
        self.assert_rejected({"lip_h": self.p["above_pcb"]}, "lip_h reaches the PCB top")

    def test_skin_as_thick_as_lid_is_rejected(self):
        self.assert_rejected({"touch_skin": self.p["lid_t"]}, "touch_skin must be thinner")

    def test_dimple_past_cavity_is_rejected(self):
        self.assert_rejected({"touch_x": self.p["pcb_l"]}, "touch dimple runs past")

    def test_cable_opening_behind_lip_is_rejected(self):
        self.assert_rejected({"port_z": self.p["above_pcb"]}, "cable opening reaches the lid lip")

    def test_pilot_wider_than_standoff_is_rejected(self):
        self.assert_rejected({"hole_d": self.p["standoff_d"]}, "hole_d must be smaller")


suite = unittest.TestSuite(
    unittest.defaultTestLoader.loadTestsFromTestCase(case)
    for case in (GeometryTest, SpreadsheetTest, ParamsTest)
)
result = unittest.TextTestRunner(stream=sys.stdout, verbosity=1).run(suite)
sys.stdout.flush()
sys.exit(0 if result.wasSuccessful() else 1)
