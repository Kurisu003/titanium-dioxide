//! Engine-independent simulation: collision and Titan movement.

pub mod collision;
pub mod pilot;
pub mod titan;

pub use glam;

#[cfg(test)]
mod tests {
    use crate::collision::CollisionWorld;
    use crate::titan::*;
    use glam::Vec3;

    fn quad(w: &mut CollisionWorld, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
        w.add(a, b, c);
        w.add(a, c, d);
    }

    fn world() -> CollisionWorld {
        let mut w = CollisionWorld::default();
        let s = 3000.0;
        quad(&mut w, Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
        // Wall at x = 1000.
        quad(&mut w, Vec3::new(1000.0, -s, 0.0), Vec3::new(1000.0, s, 0.0), Vec3::new(1000.0, s, 1000.0), Vec3::new(1000.0, -s, 1000.0));
        // A 50-unit ledge from y = 500 (steppable).
        quad(&mut w, Vec3::new(-s, 500.0, 50.0), Vec3::new(500.0, 500.0, 50.0), Vec3::new(500.0, s, 50.0), Vec3::new(-s, s, 50.0));
        quad(&mut w, Vec3::new(-s, 500.0, 0.0), Vec3::new(500.0, 500.0, 0.0), Vec3::new(500.0, 500.0, 50.0), Vec3::new(-s, 500.0, 50.0));
        w
    }

    fn run(s: &mut TitanState, input: TitanInput, w: &CollisionWorld, secs: f32) {
        let p = TitanParams::default();
        for _ in 0..(secs * 60.0) as usize {
            step(s, &input, &p, w, 1.0 / 60.0);
        }
    }

    #[test]
    fn walks_and_stops_at_wall() {
        let w = world();
        let mut s = TitanState::new(Vec3::new(0.0, 0.0, 10.0), 0.0);
        run(&mut s, TitanInput::default(), &w, 0.5);
        assert!(s.on_ground && s.pos.z.abs() < 0.01, "lands on floor: {:?}", s.pos);
        run(&mut s, TitanInput { forward: 1.0, ..Default::default() }, &w, 1.0);
        assert!((s.horizontal_speed() - 280.0).abs() < 1.0, "walk speed {}", s.horizontal_speed());
        run(&mut s, TitanInput { forward: 1.0, ..Default::default() }, &w, 6.0);
        assert!(s.pos.x < 1000.0 - 59.0 && s.pos.x > 1000.0 - 62.0, "stopped by wall at {:?}", s.pos);
    }

    #[test]
    fn sprint_and_step_up() {
        let w = world();
        let mut s = TitanState::new(Vec3::new(0.0, 0.0, 0.0), std::f32::consts::FRAC_PI_2);
        run(&mut s, TitanInput { forward: 1.0, sprint: true, yaw: std::f32::consts::FRAC_PI_2, ..Default::default() }, &w, 3.0);
        assert!((s.horizontal_speed() - 420.0).abs() < 1.0, "sprint speed {}", s.horizontal_speed());
        assert!((s.pos.z - 50.0).abs() < 0.01 && s.pos.y > 600.0, "stepped onto ledge: {:?}", s.pos);
    }

    #[test]
    fn dash_uses_power() {
        let w = world();
        let mut s = TitanState::new(Vec3::new(0.0, -1000.0, 0.0), 0.0);
        run(&mut s, TitanInput::default(), &w, 0.2);
        let p = TitanParams::default();
        step(&mut s, &TitanInput { right: 1.0, dash: true, ..Default::default() }, &p, &w, 1.0 / 60.0);
        assert_eq!(s.dashes, 1);
        assert!((s.power - 50.0).abs() < 0.5, "power {}", s.power);
        assert!(s.vel.y < -600.0, "dashing right (-Y at yaw 0): {:?}", s.vel);
        run(&mut s, TitanInput::default(), &w, 0.5);
        assert!(s.horizontal_speed() <= 350.0, "speed clamped after dash");
        run(&mut s, TitanInput::default(), &w, 2.0);
        assert!(s.on_ground);
    }

    mod pilot_tests {
        use super::quad;
        use crate::collision::CollisionWorld;
        use crate::pilot::*;
        use glam::Vec3;

        fn arena() -> CollisionWorld {
            let mut w = CollisionWorld::default();
            let s = 4000.0;
            quad(&mut w, Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
            // A long, tall wall along X at y = 300, facing -Y.
            quad(&mut w, Vec3::new(-s, 300.0, 0.0), Vec3::new(-s, 300.0, 800.0), Vec3::new(s, 300.0, 800.0), Vec3::new(s, 300.0, 0.0));
            w
        }

        fn run(s: &mut PilotState, i: PilotInput, w: &CollisionWorld, secs: f32) -> f32 {
            let p = PilotParams::default();
            let mut max_z = s.pos.z;
            for _ in 0..(secs * 120.0) as usize {
                step(s, &i, &p, w, 1.0 / 120.0);
                max_z = max_z.max(s.pos.z);
            }
            max_z
        }

        #[test]
        fn jump_and_double_jump_heights() {
            let w = arena();
            let mut s = PilotState::new(Vec3::new(0.0, 0.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            assert!(s.on_ground());
            let p = PilotParams::default();
            step(&mut s, &PilotInput { jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            let top = run(&mut s, PilotInput::default(), &w, 0.45);
            assert!((top - 60.0).abs() < 3.0, "jump apex {top}");
            step(&mut s, &PilotInput { jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            let top2 = run(&mut s, PilotInput::default(), &w, 0.6);
            assert!(top2 > 100.0, "double jump apex {top2}");
            run(&mut s, PilotInput::default(), &w, 2.0);
            assert!(s.on_ground() && s.jumps == 2);
        }

        #[test]
        fn sprint_comes_in_after_delay_and_ramp() {
            let w = arena();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            let i = PilotInput { forward: 1.0, sprint: true, ..Default::default() };
            // sprintStartDelay 0.2 s: still at walking speed shortly after pressing sprint.
            run(&mut s, i, &w, 0.3);
            assert!(s.horizontal_speed() < 175.0, "early sprint {}", s.horizontal_speed());
            // Then sprintStartDuration 0.8 s to full speed.
            run(&mut s, i, &w, 0.9);
            assert!(s.horizontal_speed() > 240.0, "sprint {}", s.horizontal_speed());
            // sprintEndDuration 0.15 s back to a walk's top speed.
            run(&mut s, PilotInput { forward: 1.0, ..Default::default() }, &w, 0.6);
            assert!(s.horizontal_speed() < 170.0, "after sprint {}", s.horizontal_speed());
        }

        #[test]
        fn sprint_and_slide() {
            let w = arena();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput { forward: 1.0, sprint: true, ..Default::default() }, &w, 2.0);
            assert!((s.horizontal_speed() - 243.75).abs() < 2.0, "sprint {}", s.horizontal_speed());
            let p = PilotParams::default();
            step(&mut s, &PilotInput { forward: 1.0, sprint: true, crouch: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Slide);
            assert!(s.horizontal_speed() > 330.0, "slide boost {}", s.horizontal_speed());
        }

        #[test]
        fn mantles_onto_a_crate() {
            let mut w = arena();
            // A 64-unit crate from x = 300 to 500 (too tall to step, low enough to mantle).
            let (x0, x1, h) = (300.0, 500.0, 64.0);
            quad(&mut w, Vec3::new(x0, -200.0, h), Vec3::new(x1, -200.0, h), Vec3::new(x1, 200.0, h), Vec3::new(x0, 200.0, h));
            quad(&mut w, Vec3::new(x0, -200.0, 0.0), Vec3::new(x0, 200.0, 0.0), Vec3::new(x0, 200.0, h), Vec3::new(x0, -200.0, h));
            let mut s = PilotState::new(Vec3::new(0.0, 0.0, 0.0), 0.0);
            run(&mut s, PilotInput { forward: 1.0, ..Default::default() }, &w, 3.0);
            assert!((s.pos.z - h).abs() < 2.0 && s.pos.x > x0, "should be on the crate: {:?} {:?}", s.pos, s.mode);
        }

        #[test]
        fn grapple_reels_in_to_the_hook() {
            let w = arena();
            let mut s = PilotState::new(Vec3::new(0.0, -600.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            assert!(s.on_ground());
            // Hook the wall 900 units away, 300 up.
            let anchor = Vec3::new(0.0, 299.0, 300.0);
            s.attach_grapple(anchor);
            let p = PilotParams::default();
            let mut arrived = false;
            for _ in 0..(3.0 * 120.0) as usize {
                step(&mut s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
                if s.grapple.is_none() {
                    arrived = true;
                    break;
                }
            }
            assert!(arrived, "should reach the hook: {:?} {:?}", s.pos, s.vel);
            // The pull slams into the wall below the hook, which knocks it loose.
            assert!(s.pos.y > 250.0 && s.pos.z > 50.0, "pulled to the wall: {:?}", s.pos);
            assert!(s.vel.z > 0.0, "impact hop: {:?}", s.vel);
            // Jumping lets go early.
            let mut s = PilotState::new(Vec3::new(0.0, -600.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            s.attach_grapple(anchor);
            run(&mut s, PilotInput::default(), &w, 0.2);
            assert!(s.grapple.is_some());
            step(&mut s, &PilotInput { jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            assert!(s.grapple.is_none() && s.mode == PilotMove::Air);
        }

        /// Jumping on the tick you land skips friction, so a chain of hops keeps its speed;
        /// waiting a few ticks on the ground loses it.
        #[test]
        fn bunny_hop_keeps_speed() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            s.vel.x = 400.0;
            let mut hops = 0;
            for _ in 0..(3.0 * 120.0) as usize {
                let jump = s.on_ground();
                step(&mut s, &PilotInput { jump, ..Default::default() }, &p, &w, 1.0 / 120.0);
                hops += jump as u32;
            }
            assert!(hops >= 4, "hops {hops}");
            assert!(s.horizontal_speed() > 395.0, "speed kept through hops: {}", s.horizontal_speed());
            // Standing on the ground bleeds it off.
            run(&mut s, PilotInput::default(), &w, 2.0);
            assert!(s.on_ground() && s.horizontal_speed() < 50.0, "friction stops you: {}", s.horizontal_speed());
        }

        /// Chained slides get slidevelocitydecay of the previous boost; after a rest the full
        /// boost is back.
        #[test]
        fn slide_boost_decays_when_chained() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
            let sprint = PilotInput { forward: 1.0, sprint: true, ..Default::default() };
            let slide = PilotInput { forward: 1.0, sprint: true, crouch: true, ..Default::default() };
            run(&mut s, sprint, &w, 2.0);
            let before = s.horizontal_speed();
            step(&mut s, &slide, &p, &w, 1.0 / 120.0);
            let first = s.horizontal_speed() - before;
            assert!((first - p.slide_boost).abs() < 2.0, "first boost {first}");
            // Hop out of the slide and slide again on landing.
            step(&mut s, &PilotInput { jump: true, ..slide }, &p, &w, 1.0 / 120.0);
            while !s.on_ground() {
                step(&mut s, &sprint, &p, &w, 1.0 / 120.0);
            }
            let before = s.horizontal_speed();
            step(&mut s, &slide, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Slide);
            let second = s.horizontal_speed() - before;
            // ... a decayed boost, which slideSpeedBoostCap stops at 400.
            let want = (before + p.slide_boost * p.slide_velocity_decay).min(before.max(p.slide_boost_cap)) - before;
            assert!((second - want).abs() < 3.0, "chained boost {second}, want {want}");
            // Rest, then the full boost again.
            run(&mut s, sprint, &w, p.slide_boost_recover + 0.5);
            let before = s.horizontal_speed();
            step(&mut s, &slide, &p, &w, 1.0 / 120.0);
            assert!((s.horizontal_speed() - before - p.slide_boost).abs() < 2.0, "boost recovered");
        }

        /// Landing with crouch held and speed from the air starts a slide without sprinting.
        #[test]
        fn slides_on_landing() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 80.0), 0.0);
            s.vel.x = 350.0;
            let crouch = PilotInput { crouch: true, ..Default::default() };
            for _ in 0..240 {
                step(&mut s, &crouch, &p, &w, 1.0 / 120.0);
                if s.mode == PilotMove::Slide {
                    break;
                }
            }
            assert_eq!(s.mode, PilotMove::Slide, "slid on landing");
        }

        /// Air strafing: momentum carries, input only bends the path (airSpeed 70).
        #[test]
        fn air_strafe_keeps_momentum() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 200.0), 0.0);
            s.mode = PilotMove::Air;
            s.vel = Vec3::new(300.0, 0.0, 0.0);
            // Holding back in the air brakes at airAcceleration only (540 u/s/s: 108 u/s in
            // 0.2 s), so momentum mostly carries.
            for _ in 0..24 {
                step(&mut s, &PilotInput { forward: -1.0, ..Default::default() }, &p, &w, 1.0 / 120.0);
            }
            assert!((s.vel.x - (300.0 - p.air_accel * 0.2)).abs() < 2.0, "air braking: {:?}", s.vel);
            // Strafing adds sideways speed up to airSpeed.
            let mut s2 = PilotState::new(Vec3::new(-2000.0, -1000.0, 200.0), 0.0);
            s2.mode = PilotMove::Air;
            s2.vel = Vec3::new(300.0, 0.0, 0.0);
            for _ in 0..24 {
                step(&mut s2, &PilotInput { right: 1.0, ..Default::default() }, &p, &w, 1.0 / 120.0);
            }
            assert!(s2.vel.y < -40.0 && s2.vel.y >= -p.air_speed - 1.0 && s2.vel.x > 299.0, "strafe: {:?}", s2.vel);
        }

        /// Wall-running carries momentum in above wallrunMaxSpeedHorizontal.
        #[test]
        fn wallrun_carries_momentum() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-1500.0, 280.0, 100.0), 0.0);
            s.mode = PilotMove::Air;
            s.vel = Vec3::new(500.0, 40.0, 50.0);
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            let mut ran = false;
            for _ in 0..60 {
                step(&mut s, &fwd, &p, &w, 1.0 / 120.0);
                ran |= matches!(s.mode, PilotMove::WallRun { .. });
            }
            assert!(ran, "wallrun started: {:?}", s.mode);
            assert!(s.horizontal_speed() > 480.0, "entry speed kept: {}", s.horizontal_speed());
        }

        /// With the wall-hang kit, aiming on a wall hangs there until the limit.
        #[test]
        fn wall_hang() {
            let w = arena();
            let p = PilotParams { wallhang_on_ads: true, ..Default::default() };
            let mut s = PilotState::new(Vec3::new(-1500.0, 280.0, 100.0), 0.0);
            s.mode = PilotMove::Air;
            s.vel = Vec3::new(300.0, 40.0, 50.0);
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            for _ in 0..30 {
                step(&mut s, &fwd, &p, &w, 1.0 / 120.0);
            }
            assert!(matches!(s.mode, PilotMove::WallRun { .. }), "{:?}", s.mode);
            let hang = PilotInput { ads: true, ..fwd };
            step(&mut s, &hang, &p, &w, 1.0 / 120.0);
            let at = s.pos;
            for _ in 0..240 {
                step(&mut s, &hang, &p, &w, 1.0 / 120.0);
            }
            assert!(matches!(s.mode, PilotMove::WallHang { .. }) && s.pos.distance(at) < 0.01, "hanging still: {:?}", s.mode);
            for _ in 0..(p.wallrun_hang_time * 120.0) as usize {
                step(&mut s, &hang, &p, &w, 1.0 / 120.0);
            }
            assert!(!matches!(s.mode, PilotMove::WallHang { .. }), "drops after wallrun_hangTimeLimit: {:?}", s.mode);
            // Without the kit, aiming doesn't hang (the campaign's "ADS" type).
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-1500.0, 280.0, 100.0), 0.0);
            s.mode = PilotMove::Air;
            s.vel = Vec3::new(300.0, 40.0, 50.0);
            for _ in 0..60 {
                step(&mut s, &PilotInput { ads: true, ..fwd }, &p, &w, 1.0 / 120.0);
            }
            assert!(!matches!(s.mode, PilotMove::WallHang { .. }));
        }

        /// Falls faster than impactSpeed are hard landings (reported once; no fall damage).
        #[test]
        fn hard_landing() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(0.0, -1000.0, 40.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 1.0);
            assert!(s.take_hard_landing(&p).is_none(), "a short drop is soft");
            let mut s = PilotState::new(Vec3::new(0.0, -1000.0, 400.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 2.0);
            let v = s.take_hard_landing(&p);
            assert!(v.is_some_and(|v| v > p.impact_speed), "hard landing {v:?}");
            assert!(s.take_hard_landing(&p).is_none(), "reported once");
        }

        #[test]
        fn wallrun_and_wall_jump() {
            let w = arena();
            // Start beside the wall, run parallel to it, jump toward it.
            let mut s = PilotState::new(Vec3::new(-1500.0, 200.0, 0.0), 0.3);
            run(&mut s, PilotInput { forward: 1.0, sprint: true, yaw: 0.3, ..Default::default() }, &w, 1.0);
            let p = PilotParams::default();
            step(&mut s, &PilotInput { forward: 1.0, sprint: true, jump: true, yaw: 0.3, ..Default::default() }, &p, &w, 1.0 / 120.0);
            let mut ran = false;
            for _ in 0..240 {
                step(&mut s, &PilotInput { forward: 1.0, sprint: true, yaw: 0.0, ..Default::default() }, &p, &w, 1.0 / 120.0);
                if let PilotMove::WallRun { time, .. } = s.mode {
                    ran = time > 0.5;
                    if ran {
                        break;
                    }
                }
            }
            assert!(ran, "should wallrun for over 0.5 s, mode {:?} pos {:?}", s.mode, s.pos);
            assert!(s.pos.z > 20.0, "off the ground while wallrunning: {:?}", s.pos);
            assert!(s.horizontal_speed() > 250.0, "wallrun speed {}", s.horizontal_speed());
            step(&mut s, &PilotInput { forward: 1.0, jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Air);
            assert!(s.vel.y < -150.0 && s.vel.z > 200.0, "jumped off the wall: {:?}", s.vel);
        }
    }

    #[test]
    fn zipline_ride() {
        use crate::pilot::*;
        let w = world();
        let p = PilotParams::default();
        let line = Zipline { a: Vec3::new(0.0, 0.0, 800.0), b: Vec3::new(3000.0, 0.0, 600.0), sag: 100.0, detach: 128.0 };
        // Falling into the line from just under it, facing along it: grabbed.
        let mut s = PilotState::new(Vec3::new(500.0, 0.0, 800.0 - 72.0 - 70.0), 0.0);
        assert!(s.try_zipline(&[line], false, &p));
        let input = PilotInput::default();
        let mut t = 0.0;
        let mut top = 0.0f32;
        while s.zipline.is_some() && t < 10.0 {
            step(&mut s, &input, &p, &w, 1.0 / 60.0);
            top = top.max(s.vel.length());
            t += 1.0 / 60.0;
        }
        assert!(s.zipline.is_none(), "let go at the end");
        assert!((top - p.zipline.speed).abs() < 1.0, "rode at ziplineSpeed: {top}");
        assert!(s.pos.x > 2700.0 && s.pos.x < 2900.0, "let go before the end: {}", s.pos.x);
        // No re-grab during the cooldown.
        assert!(!s.try_zipline(&[line], true, &p));
    }
}
