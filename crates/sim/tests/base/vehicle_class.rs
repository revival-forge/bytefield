//! What sort of vehicle a template is, for vanilla and DC. Skipped without the real data (`data/game`).
use crate::support::vehicle_class::*;

use bytefield_sim::bot_api::VehicleClass::*;

#[test]
fn vanilla() {
    check("bf1942", "Midway", &[("Corsair", Plane), ("fletcher", Ship), ("Gato", Submarine), ("Defgun", Fixed)]);
    check("bf1942", "El_Alamein", &[("Sherman", Tank), ("Willy", Car), ("Bf109", Plane)]);
}
