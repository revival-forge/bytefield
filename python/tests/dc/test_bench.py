"""The test bench on Desert Combat Final's templates (`import bytefield`)."""

import pytest

bytefield = pytest.importorskip("bytefield")
from tests.layers import GAME  # noqa: E402


def test_vehicle_models():
    w = bytefield.World(str(GAME), "DC_Final", "DC_Basrahs_Edge")
    ah64 = w.vehicle("AH64")
    assert ah64["kind"] == "PlayerControlObject"
    assert ah64["params"]["mass"] == 2500

    def engines(c):
        own = [c] if c["kind"].lower() == "engine" else []
        return own + [e for k in c["children"] for e in engines(k)]

    # DC builds its helicopters out of plane engines (no rotor type in the engine)
    assert [e["params"].get("setenginetype") for e in engines(ah64)].count("c_ETPlane") == 5
    with pytest.raises(KeyError):
        w.vehicle("NoSuchThing")
