
from bytefield_tools.con_scan import iter_lines, scan, split_args, to_json, value_kind
from tests import layers  # noqa: E402

FIXTURES = layers.FIXTURES / "mod"


def test_split_args_keeps_quoted_spaces():
    assert split_args('ObjectTemplate.setVehicleIcon "Vehicle/Icon xwing.tga" 1') == [
        "ObjectTemplate.setVehicleIcon", "Vehicle/Icon xwing.tga", "1"]


def test_comments_and_blocks_are_dropped():
    text = "rem hello\nrem*****\nbeginrem\nA.b 1\nendrem\n\nA.c 2\nremove.me 3\n"
    assert [(ln.command, ln.args) for ln in iter_lines(text)] == [("A.c", ["2"]), ("remove.me", ["3"])]


def test_value_kinds():
    assert [value_kind(a) for a in ["1", "-2.5", "0/0/-10", "1/2", "v_arg1", "foo.tga", "XWing"]] == [
        "int", "float", "vec3", "vec2", "var", "path", "name"]


def test_scan_fixture():
    inv = scan(FIXTURES)
    j = to_json(inv)
    assert j["files"] == 7
    assert j["creates"] == {
        "geometrytemplate.standardmesh": 1, "objecttemplate.engine": 1, "objecttemplate.playercontrolobject": 1}
    # properties set after `active` in another file count for the right type
    assert j["props"]["objecttemplate.engine"] == {"setmaxrotation": 1, "setminrotation": 1, "settorque": 1}
    assert j["props"]["objecttemplate.playercontrolobject"]["hasmobilephysics"] == 1
    assert j["keywords"] == {"endif": 2, "if": 2, "include": 1, "run": 4, "var": 1}
    assert j["conditions"] == {"v_arg1 == host": 1, "v_mode == GPM_CQ": 1}
    # Physics: next to the script; Conquest/ControlPoints: from the level dir (an ancestor)
    assert j["run_resolution"] == {"ancestor": 1, "file": 3, "unresolved": 1}
    assert j["stray"] == 1
    # an .inc runs in its includer's scope: on its own it has no active ObjectTemplate,
    # and GeometryTemplate.setActive switches like `active` does
    assert inv.no_active == {"objecttemplate.mass": 1}
    assert j["props"]["geometrytemplate.standardmesh"] == {"shadowprecision": 1}
    assert inv.shapes["objecttemplate.setminrotation"] == {("vec3",): 1}
    assert inv.shapes["game.getgameplaymode"] == {(): 1}  # `-> v_mode` is not an argument
