//! Tests of the play module: scripts, armour effects, key presses, and the ones on the real data.

use crate::play::*;

#[test]
fn scripts_parse() {
    let s = parse_script("0-60:w;60-90:w,mx=2; 100:e").unwrap();
    assert_eq!(s.len(), 3);
    assert_eq!((s[1].from, s[1].to, s[1].mouse.x), (60, 90, 2.0));
    assert_eq!((s[2].from, s[2].to, s[2].keys[0].as_str()), (100, 101, "e"));
    assert!(parse_script("x:w").is_err());
}

#[test]
fn the_armor_picks_its_effect() {
    // the T34's: smoke at 50, fire at 12, the blast and scrap at 0, the water's at −1
    let mut e = std::collections::BTreeMap::new();
    for (k, n) in [(50, "e_PanzDamage"), (12, "e_PanzFire"), (0, "e_ExplGas"), (-1, "WaterWaterExplosion")] {
        e.insert(k, vec![(n.to_string(), [0.0f32; 3])]);
    }
    assert_eq!(armor_threshold(&e, 100.0, None), None);
    assert_eq!(armor_threshold(&e, 50.0, None), Some(50));
    assert_eq!(armor_threshold(&e, 30.2, None), Some(50));
    assert_eq!(armor_threshold(&e, 12.4, None), Some(12));
    assert_eq!(armor_threshold(&e, 3.0, None), Some(12));
    assert_eq!(armor_threshold(&e, 0.0, Some(0)), Some(0));
    assert_eq!(armor_threshold(&e, 0.0, Some(-1)), Some(-1), "destroyed in the water");
    e.remove(&-1);
    assert_eq!(armor_threshold(&e, 0.0, Some(-1)), Some(0), "without one, that of 0");
}

#[test]
fn some_keys_act_on_the_press() {
    let mut now = Held::default();
    now.keys.insert("e".into());
    now.keys.insert("w".into());
    let get = |a: &[(usize, f32)], i: usize| a.iter().find(|x| x.0 == i).unwrap().1;
    let first = axes(&now, &Held::default());
    assert_eq!(get(&first, bytefield_sim::entry::USE), 1.0);
    assert_eq!(get(&first, axis::THROTTLE), 1.0);
    let held = axes(&now, &now);
    assert_eq!(get(&held, bytefield_sim::entry::USE), 0.0);
    assert_eq!(get(&held, axis::THROTTLE), 1.0);
}

pub(crate) mod with_data {
    use crate::play::*;

    /// The game's files, when they are here (the tests on the real data skip without them).
    pub(crate) fn world(level: &str) -> Option<&'static World> {
        let game = bytefield_testdata::game();
        game.is_dir().then(|| &*Box::leak(Box::new(World::load(&game, "bf1942", level, "conquest").expect("level"))))
    }

    /// `Parachute.skn` hangs from `Parachute.ske`: folded when it opens, and the canopy well over the soldier after.
    #[test]
    fn the_parachute_opens_on_its_own_skeleton() {
        let Some(w) = world("Market_Garden") else { return };
        let sim = run(w, 2, "1:", 2);
        let v = &sim.vehicles[sim.players[0].soldier.unwrap()];
        let d = model::draws(&w.db, &v.template, &v.craft.parts).into_iter().find(|d| d.parachute && d.skin.is_some()).expect("its parachute");
        assert!(!d.soldier);
        let (ske, _) = d.track.clone().expect("its skeleton");
        let file = Arc::new(bytefield_formats::anim::Skeleton::parse(&w.vfs.read(&ske).unwrap()).unwrap());
        let skins = skin::load(&w.vfs, &d.file, d.skin.as_deref().unwrap(), &file, false).expect("all of it hangs from that skeleton");
        let machine = &v.soldier().unwrap().machine;
        let top = |abs: &[bytefield_sim::skeleton::M4]| skins.iter().flat_map(|s| skin::positions(s, abs)).map(|p| p[1]).fold(f32::MIN, f32::max);
        let mut chute = skin::Chute::new(file);
        let shut = top(chute.advance(machine, 1.0));
        let opening = top(chute.advance(machine, 1.0));
        let open = top(chute.advance(machine, 10.0));
        let later = top(chute.advance(machine, 0.5));
        assert!(shut < 3.0 && shut < opening && opening < open, "{shut} {opening} {open}");
        assert!(open > 10.0 && (open - later).abs() < 1.0, "{open} {later}");
    }

    /// Runs a script headless: the player's soldier each tick.
    fn run(world: &'static World, team: u32, script: &str, ticks: u64) -> Sim<'static> {
        let mut sim = Sim::new(world, TICK);
        sim.round.playing = true;
        sim.start_conquest();
        let p = sim.add_player(team, "USSoldier", false);
        let script = parse_script(script).unwrap();
        let mut before = Held::default();
        for t in 0..ticks {
            let mut h = Held::default();
            for s in script.iter().filter(|s| (s.from..s.to).contains(&t)) {
                h.keys.extend(s.keys.iter().cloned());
                h.mouse += s.mouse;
            }
            let a = axes(&h, &before);
            if let Some(i) = sim.players[p].soldier
                && let Some(m) = sim.vehicles[i].soldier_mut()
            {
                m.set_input(&a);
            }
            before = h;
            sim.step();
        }
        sim
    }

    #[test]
    fn walk_to_the_m10_get_in_and_drive() {
        let Some(w) = world("El_Alamein") else { return };
        // it turns to the M10 (51°: 17 ticks of 3°) and runs to it, tapping E (once in it, no more)
        let e: String = (30..=70).step_by(4).map(|t| format!("{t}:e;")).collect();
        let sim = run(w, 2, &format!("0-17:mx=1;17-75:w;{e}"), 80);
        let s = sim.players[0].soldier.unwrap();
        let k = sim.vehicles[s].entry.inside.expect("in the M10");
        assert!(sim.vehicles[k].template.eq_ignore_ascii_case("m10"));
        let start = sim.vehicles[k].craft.body.pos;
        // then W: the tank goes (a second of throttle; E again gets out)
        let sim = run(w, 2, &format!("0-17:mx=1;17-75:w;{e}90-150:w;160:e"), 175);
        let d = (0..3).map(|j| (sim.vehicles[k].craft.body.pos[j] - start[j]).powi(2)).sum::<f32>().sqrt();
        assert!(d > 2.0, "the M10 moved {d} m");
        assert_eq!(sim.vehicles[s].entry.inside, None);
    }

    /// The view between two ticks (the mouse not sent yet) is where the next tick leaves it:
    /// turning (3° a unit) and looking up and down (2.5°), standing still.
    #[test]
    fn the_view_is_where_the_next_tick_leaves_it() {
        let Some(w) = world("El_Alamein") else { return };
        let mut sim = run(w, 2, "", 150);
        let s = sim.players[0].soldier.unwrap();
        let view = |sim: &Sim, look: [f32; 2]| {
            let v = &sim.vehicles[s];
            v.soldier().unwrap().view(&v.craft.body, look).0
        };
        let angle = |a: [f32; 3], b: [f32; 3]| (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0).acos().to_degrees();
        let mut before = Held::default();
        for look in [[1.5f32, 0.0], [0.0, -2.0], [-2.5, 1.0], [0.0, 0.0]] {
            let was = view(&sim, [0.0, 0.0]);
            let predicted = view(&sim, look);
            let h = Held { keys: HashSet::new(), mouse: Vec2::from(look) };
            let a = axes(&h, &before);
            sim.vehicles[s].soldier_mut().unwrap().set_input(&a);
            before = h;
            sim.step();
            let after = view(&sim, [0.0, 0.0]);
            let turned = angle(was[2], predicted[2]);
            let expect = (look[0] * 3.0).hypot(look[1] * 2.5);
            assert!((turned - expect).abs() < 0.2, "{look:?}: turned {turned}°, not {expect}°");
            for k in 0..3 {
                assert!(angle(predicted[k], after[k]) < 0.01, "{look:?}: axis {k} {:?} → {:?}", predicted[k], after[k]);
            }
        }
    }

    /// The weapon in the hand: its stock a few centimetres from the right hand, the hand about
    /// the chest, in front of the soldier.
    #[test]
    fn the_rifle_is_in_the_right_hand() {
        let Some(w) = world("El_Alamein") else { return };
        let sim = run(w, 2, "", 150);
        let s = &sim.vehicles[sim.players[0].soldier.unwrap()];
        let m = s.soldier().unwrap();
        let name = &m.weapon.as_ref().expect("a weapon").template.name;
        let model = weapon::load(&w.db, &w.vfs, name).expect("its model");
        let sk = m.posed_skeleton().unwrap();
        let hand = sk.abs[sk.file.index(weapon::HAND).unwrap()];
        let base = model.piece(&model.bones(&hand, &model.rel(None)), 0);
        eprintln!("{name}: hand {:?} base {:?}", &hand[12..15], &base[12..15]);
        let d = (0..3).map(|k| (base[12 + k] - hand[12 + k]).powi(2)).sum::<f32>().sqrt();
        assert!(d < 0.2, "{d}");
        // the mesh's long axis in the soldier's frame (forward: +z)
        let sm = bytefield_formats::sm::parse(&w.vfs.read(&format!("standardmesh/{}.sm", model.pieces[0].file)).unwrap()).unwrap();
        let ps: Vec<[f32; 3]> = sm.lods[0].materials.iter().flat_map(|m| m.positions.iter().copied()).collect();
        let lo = ps.iter().fold([f32::MAX; 3], |a, v| [0, 1, 2].map(|k| a[k].min(v[k])));
        let hi = ps.iter().fold([f32::MIN; 3], |a, v| [0, 1, 2].map(|k| a[k].max(v[k])));
        eprintln!("mesh box {lo:?} {hi:?}; base rows {:?} {:?} {:?}", &base[0..3], &base[4..7], &base[8..11]);
        // aiming, standing still: the barrel (the mesh's +z, its long side) ahead, its y up
        assert!(hi[2] > -lo[2] && hi[2] - lo[2] > hi[0] - lo[0] && hi[2] - lo[2] > hi[1] - lo[1]);
        assert!(base[10] > 0.8 && base[5] > 0.8, "{base:?}");
        assert!(hand[13] > -0.5 && hand[13] < 0.8 && hand[14] > 0.0, "{:?}", &hand[12..15]);
    }

    /// After a shot the No4 works its bolt (`Ub_StandFireNo4` → `Ub_StandReloadNo4`, whose
    /// weapon state is `WeaponReloadNo4`): its own animation moves a piece (the bolt, `Load`)
    /// away from its place.
    #[test]
    fn the_rifle_works_its_bolt() {
        let Some(w) = world("El_Alamein") else { return };
        let mut sim = run(w, 2, "40:lmb", 41);
        let s = sim.players[0].soldier.unwrap();
        let name = sim.vehicles[s].soldier().unwrap().weapon.as_ref().unwrap().template.name.clone();
        let model = weapon::load(&w.db, &w.vfs, &name).unwrap();
        let still = model.bones(&bytefield_sim::skeleton::IDENTITY, &model.rel(None));
        let mut moved = 0.0f32;
        let mut animated = false;
        for _ in 0..90 {
            sim.step();
            let m = sim.vehicles[s].soldier().unwrap();
            let Some(a) = weapon_animation(m) else { continue };
            animated = true;
            let now = model.bones(&bytefield_sim::skeleton::IDENTITY, &model.rel(Some(a)));
            for k in 0..model.pieces.len() {
                let (p, q) = (model.piece(&still, k), model.piece(&now, k));
                moved = moved.max((12..15).map(|j| (p[j] - q[j]).powi(2)).sum::<f32>().sqrt());
            }
        }
        assert!(animated, "no weapon animation after the shot");
        assert!(moved > 0.05, "the pieces moved {moved} m");
    }

    /// A track follows its wheels: settled, the M10's springs are pushed up some 14 cm, and the
    /// lowest point of its left track posed so is on the ground (within 6 cm; at rest it was
    /// under it by about the springs' travel).
    #[test]
    fn the_tracks_stand_on_the_ground() {
        let Some(w) = world("El_Alamein") else { return };
        let sim = run(w, 2, "", 90);
        let v = sim.vehicles.iter().find(|v| v.template.eq_ignore_ascii_case("m10")).unwrap();
        let d = model::draws(&w.db, &v.template, &v.craft.parts).into_iter().find(|d| d.track.is_some()).expect("a track");
        let (ske, bones) = d.track.clone().unwrap();
        let file = bytefield_formats::anim::Skeleton::parse(&w.vfs.read(&ske).unwrap()).unwrap();
        let skins = skin::load(&w.vfs, &d.file, d.skin.as_deref().unwrap(), &file, true).expect("its skin");
        let rows = v.craft.body.rows;
        let lowest = |moved: bool| {
            let mut abs: Vec<bytefield_sim::skeleton::M4> = file.bones.iter().map(bytefield_sim::skeleton::bone_matrix).collect();
            if moved {
                for (b, (name, n)) in abs.iter_mut().zip(&bones) {
                    let o = spring_offset(v, name, *n).unwrap();
                    for (k, row) in rows.iter().enumerate() {
                        b[12 + k] += o[0] * row[0] + o[1] * row[1] + o[2] * row[2];
                    }
                }
            }
            let place = assets::frame_matrix(&rows, v.craft.body.pos).mul(&d.local);
            let mut low = f64::MAX;
            for s in &skins {
                let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
                skin::pose(s, &abs, &mut mesh);
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
                for q in p {
                    // back from Bevy's mirror, then to the world
                    let l = [q[0] as f64, q[1] as f64, -q[2] as f64];
                    let m = &place.0;
                    let wp: Vec<f64> = (0..3).map(|c| l[0] * m[0][c] + l[1] * m[1][c] + l[2] * m[2][c] + m[3][c]).collect();
                    low = low.min(wp[1] - w.height_at(wp[0], wp[2]).unwrap());
                }
            }
            low
        };
        let (rest, moved) = (lowest(false), lowest(true));
        eprintln!("the track's lowest point over the ground: at rest {rest:.3} m, moved {moved:.3} m");
        assert!(moved.abs() < 0.06 && rest < moved - 0.08, "{rest} {moved}");
    }

    /// A bazooka fired at the ground hits it and explodes; the effect of its material against
    /// the sand's (`MaterialManager.setEffectTemplate`: `e_ExplBazooka`) loads.
    #[test]
    fn an_explosion_has_its_effect() {
        let Some(w) = world("El_Alamein") else { return };
        let mut sim = Sim::new(w, TICK);
        sim.round.playing = true;
        sim.start_conquest();
        let p = sim.add_player(2, "USSoldier", false);
        sim.players[p].kit = 2;
        let script = parse_script("0-6:my=3;6-40:;40-42:lmb;42-90:").unwrap();
        let mut before = Held::default();
        for t in 0..90 {
            let mut h = Held::default();
            for s in script.iter().filter(|s| (s.from..s.to).contains(&t)) {
                h.keys.extend(s.keys.iter().cloned());
                h.mouse += s.mouse;
            }
            let a = axes(&h, &before);
            if let Some(i) = sim.players[p].soldier
                && let Some(m) = sim.vehicles[i].soldier_mut()
            {
                m.set_input(&a);
            }
            before = h;
            sim.step();
        }
        assert!(!sim.journal.explosions.is_empty(), "no explosion");
        let (m, hit) = sim.journal.impacts.first().expect("a hit");
        let name = w.materials().effects.get(&(*m as i64, hit.material as i64)).cloned();
        eprintln!("hit of material {m} on {}: {name:?}", hit.material);
        let name = name.expect("an effect for it");
        assert!(crate::effects::load(&w.db, &name).is_some(), "{name} does not load");
    }

    #[test]
    fn the_left_button_fires_the_rifle() {
        let Some(w) = world("El_Alamein") else { return };
        let sim = run(w, 2, "10-12:lmb", 20);
        let s = sim.players[0].soldier.unwrap();
        assert_eq!(sim.journal.fired.iter().filter(|f| f.1 == s).count(), 1);
    }
}

mod skin_data {
    use crate::play::*;

    #[test]
    fn a_skinned_body_stands_about_the_soldier() {
        let game = bytefield_testdata::require!(Base);
        let w: &'static World = Box::leak(Box::new(World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap()));
        let mut sim = Sim::new(w, TICK);
        let i = sim.spawn_vehicle("GermanSoldier", [500.0, 100.0, 500.0], [0.0; 3]).unwrap();
        for _ in 0..3 {
            sim.step();
        }
        let m = sim.vehicles[i].soldier().unwrap();
        let sk = m.posed_skeleton().unwrap();
        let skins = skin::load(&w.vfs, "GerBody", "animations/GerBody.skn", &sk.file, false).expect("skin");
        let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
        for s in &skins {
            skin::pose(s, &sk.abs, &mut mesh);
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            let lo = p.iter().fold([f32::MAX; 3], |a, v| [0, 1, 2].map(|k| a[k].min(v[k])));
            let hi = p.iter().fold([f32::MIN; 3], |a, v| [0, 1, 2].map(|k| a[k].max(v[k])));
            eprintln!("{lo:?} {hi:?}");
            // in the soldier's frame: feet about 1 m under its centre, the head 0.8 m over it
            assert!(lo[1] > -1.3 && hi[1] < 1.2 && hi[1] > 0.3, "{lo:?} {hi:?}");
        }
    }
}
mod wheels {
    /// A car's engine rolls with the throttle; the jeep's wheels hang from it and stay under the
    /// body.
    #[test]
    fn the_jeeps_wheels_stay_under_it() {
        let Some(w) = crate::play::with_data::world("El_Alamein") else { return };
        let mut sim = bytefield_sim::Sim::new(w, crate::play::TICK);
        sim.round.playing = true;
        sim.start_conquest();
        sim.step();
        let v = sim.vehicles.iter_mut().find(|v| v.template == "Willy").expect("a Willy");
        for p in v.craft.parts.iter_mut().filter(|p| matches!(p.kind, bytefield_sim::craft::Kind::Engine(_))) {
            p.bundle.as_mut().unwrap().angle[2] = 90.0;
        }
        let (body, frames) = (v.craft.body.pos, v.craft.frames());
        let wheels: Vec<_> = crate::model::draws(&w.db, "Willy", &v.craft.parts).into_iter().filter(|d| d.file.to_ascii_lowercase().contains("whe")).collect();
        assert_eq!(wheels.len(), 4, "{:?}", wheels.iter().map(|d| &d.file).collect::<Vec<_>>());
        for d in wheels {
            let m = match d.part {
                Some(k) => crate::assets::frame_matrix(&frames[k].child_rows, frames[k].pos),
                None => crate::assets::frame_matrix(&v.craft.body.rows, body),
            };
            let at = m.mul(&d.local).0[3];
            let flat = ((at[0] as f32 - body[0]).powi(2) + (at[2] as f32 - body[2]).powi(2)).sqrt();
            assert!((at[1] as f32) < body[1] + 0.2 && flat < 2.0, "{} at {at:?}, the body at {body:?}", d.file);
        }
    }
}

mod arms_ease_tests {
    use crate::play::arms_ease;

    #[test]
    fn eases_like_the_client_and_lands() {
        let (p, f) = arms_ease(([0.0; 3], 1.0), ([1.0, 2.0, -4.0], 0.6));
        assert_eq!(p, [0.25, 0.5, -1.0]);
        assert!((f - 0.88).abs() < 1e-6);
        assert_eq!(arms_ease(([0.0; 3], 0.6005), ([0.0; 3], 0.6)).1, 0.6);
    }
}
