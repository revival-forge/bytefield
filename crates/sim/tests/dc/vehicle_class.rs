//! What sort of vehicle a template is, for vanilla and DC. Skipped without the real data (`data/game`).
use crate::support::vehicle_class::*;

use bytefield_sim::bot_api::VehicleClass::*;

#[test]
fn desert_combat() {
    for m in crate::support::dc_mods() {
        if crate::support::has_level(&m, "DC_First_Light") {
            check(&m, "DC_First_Light", &[("a10", Plane), ("f-15c", Plane), ("uh-60", Helicopter), ("ah64", Helicopter), ("mi24d", Helicopter), ("md-500", Helicopter), ("m1a1", Tank), ("t72", Tank), ("bmp2", Tank), ("humvee", Car), ("zpu-4", Fixed)]);
        }
        check(&m, "DC_No_Fly_Zone", &[("AV-8H", Plane), ("F-14B", Plane)]);
    }
}
