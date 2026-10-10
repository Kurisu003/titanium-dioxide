//! Engine-independent simulation: collision, and Pilot and Titan movement.

pub mod collision;
pub mod pilot;
pub mod titan;

pub use glam;

/// Source's SimpleSpline, 3t^2 - 2t^3: eases 0..1 in and out (the crouch view, mantles).
pub(crate) fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

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
    fn walking_into_a_wall_at_a_steep_angle_slides_along_it() {
        // 66 degrees into the wall at x = 1000: Friction keeps the walk speed's share along
        // the wall (280 x cos 66 = 114, client.dll 0x1801f81bf) instead of braking it away.
        let w = world();
        let yaw = 24f32.to_radians();
        let mut s = TitanState::new(Vec3::new(900.0, -1500.0, 0.0), yaw);
        let input = TitanInput { forward: 1.0, yaw, ..Default::default() };
        run(&mut s, input, &w, 3.0);
        let y0 = s.pos.y;
        run(&mut s, input, &w, 1.0);
        let share = 280.0 * 66f32.to_radians().cos();
        assert!(s.pos.x > 1000.0 - 62.0, "at the wall: {:?}", s.pos);
        assert!(s.pos.y - y0 > share - 2.0, "slides at {} along the wall, share {share}", s.pos.y - y0);
    }

    #[test]
    fn landing_records_impact_speed() {
        let w = world();
        // A 120-unit fall (viewkickFallDistMax) lands at about sqrt(2 * 750 * 120) = 424 u/s.
        let mut s = TitanState::new(Vec3::new(0.0, -1000.0, 120.0), 0.0);
        run(&mut s, TitanInput::default(), &w, 1.0);
        assert_eq!(s.landings, 1);
        assert!((s.land_speed - 424.0).abs() < 15.0, "impact speed {}", s.land_speed);
        // Walking on flat ground lands no more.
        run(&mut s, TitanInput { forward: 1.0, ..Default::default() }, &w, 1.0);
        assert_eq!(s.landings, 1);
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
    fn turning_while_sprinting_bends_the_path() {
        // Sprint, then turn holding forward for a second: friction slows only the sideways
        // part, so the velocity follows the facing (one rate toward the wanted velocity lagged
        // 26 degrees at 45 deg/s and slid nearly straight on at 180).
        let w = world();
        let p = TitanParams::default();
        for (rate, max_lag) in [(45f32, 1.0f32), (180.0, 5.0)] {
            let mut s = TitanState::new(Vec3::new(-2500.0, -2500.0, 0.0), 0.0);
            run(&mut s, TitanInput { forward: 1.0, sprint: true, ..Default::default() }, &w, 3.0);
            let mut yaw = 0.0f32;
            for _ in 0..60 {
                yaw += rate.to_radians() / 60.0;
                step(&mut s, &TitanInput { forward: 1.0, sprint: true, yaw, ..Default::default() }, &p, &w, 1.0 / 60.0);
            }
            let lag = (yaw - s.vel.y.atan2(s.vel.x)).to_degrees();
            assert!(lag.abs() < max_lag, "{rate} deg/s: facing vs movement lags {lag} degrees");
        }
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
        assert!((s.vel.y + 685.0).abs() < 1.0, "dashing right (-Y at yaw 0) at dodgeSpeed: {:?}", s.vel);
        // Airborne for dodgeDuration, keeping the dash speed (no friction, no clamp).
        run(&mut s, TitanInput::default(), &w, 0.25);
        assert!(!s.on_ground && (s.horizontal_speed() - 685.0).abs() < 2.0, "dash holds: {}", s.horizontal_speed());
        // dodgeInterval (0.2 s) after the first started, the second needs the other 50 power.
        step(&mut s, &TitanInput { forward: 1.0, dash: true, ..Default::default() }, &p, &w, 1.0 / 60.0);
        assert_eq!(s.dashes, 2);
        step(&mut s, &TitanInput { forward: 1.0, dash: true, ..Default::default() }, &p, &w, 1.0 / 60.0);
        assert_eq!(s.dashes, 2, "out of power");
        // Not faster than the dash, the old velocity is replaced: 685 forward.
        assert!((s.vel.x - 685.0).abs() < 5.0 && s.vel.y.abs() < 5.0, "second dash {:?}", s.vel);
        // Then the ground brakes it at sprintDeceleration down to walk speed and to a stop.
        run(&mut s, TitanInput::default(), &w, 4.0);
        assert!(s.on_ground && s.horizontal_speed() < 1.0);
    }

    #[test]
    fn dash_steers_with_keyboard_grace() {
        // Dash forward, then hold right: within jump_keyboardgraceperiodmax the velocity turns
        // toward the keys (the per-tick limit cuts the corner, so a sharp turn loses some speed).
        let w = world();
        let p = TitanParams::default();
        let mut s = TitanState::new(Vec3::new(0.0, -1000.0, 0.0), 0.0);
        run(&mut s, TitanInput::default(), &w, 0.2);
        step(&mut s, &TitanInput { forward: 1.0, dash: true, ..Default::default() }, &p, &w, 1.0 / 60.0);
        run(&mut s, TitanInput { right: 1.0, ..Default::default() }, &w, 0.1);
        assert!(s.vel.y < -500.0 && s.vel.x.abs() < 50.0, "turned right: {:?}", s.vel);
    }

    #[test]
    fn crouch_stance() {
        let mut w = world();
        // A 200-unit-high roof over x 1500..2500 (the wall at x = 1000 is behind BT).
        quad(&mut w, Vec3::new(1500.0, -2000.0, 200.0), Vec3::new(1500.0, -1000.0, 200.0), Vec3::new(2500.0, -1000.0, 200.0), Vec3::new(2500.0, -2000.0, 200.0));
        let p = TitanParams::default();
        let mut s = TitanState::new(Vec3::new(0.0, -1500.0, 0.0), 0.0);
        run(&mut s, TitanInput::default(), &w, 0.2);
        let stand_eye = s.eye(&p).z;
        assert!((stand_eye - 185.0).abs() < 0.01);
        // Crouching: the eye eases down to 125 over 0.4 s, the speed is the crouch stance's, no
        // sprint.
        let crouch = TitanInput { forward: 1.0, crouch: true, sprint: true, ..Default::default() };
        run(&mut s, crouch, &w, 0.2);
        let mid = s.eye(&p).z;
        assert!(s.crouched && mid < 185.0 && mid > 125.0, "eye half way down: {mid}");
        run(&mut s, crouch, &w, 1.5);
        assert!((s.eye(&p).z - 125.0).abs() < 0.01, "crouch eye {}", s.eye(&p).z);
        assert!(!s.sprinting);
        assert!((s.horizontal_speed() - 150.0).abs() < 1.0, "crouch speed {}", s.horizontal_speed());
        // Walk crouched under the roof, let go of crouch: no room to stand.
        let mut s = TitanState::new(Vec3::new(2000.0, -1500.0, 0.0), 0.0);
        s.crouched = true;
        s.duck = 1.0;
        run(&mut s, TitanInput::default(), &w, 0.5);
        assert!(s.crouched, "stays crouched under the roof");
        // Out in the open it stands up, the eye rising over 0.2 s.
        let mut s = TitanState::new(Vec3::new(0.0, -1500.0, 0.0), 0.0);
        s.crouched = true;
        s.duck = 1.0;
        run(&mut s, TitanInput::default(), &w, 0.25);
        assert!(!s.crouched && (s.eye(&p).z - 185.0).abs() < 0.01, "stood up: eye {}", s.eye(&p).z);
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
            // jumpheight 60 at sv_gravity, falling at sv_gravity * gravityscale: 60 / 0.8 high.
            let top = run(&mut s, PilotInput::default(), &w, 0.5);
            assert!((top - 75.0).abs() < 3.0, "jump apex {top}");
            step(&mut s, &PilotInput { jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            let top2 = run(&mut s, PilotInput::default(), &w, 0.6);
            assert!(top2 > 100.0, "double jump apex {top2}");
            run(&mut s, PilotInput::default(), &w, 2.0);
            assert!(s.on_ground() && s.jumps == 2);
        }

        /// Sprint speed follows the ground acceleration; the sprint view (sprint_frac) waits
        /// sprintStartDelay, comes in over sprintStartDuration and goes over sprintEndDuration.
        #[test]
        fn sprint_speed_and_view_timing() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput { forward: 1.0, ..Default::default() }, &w, 1.0);
            let i = PilotInput { forward: 1.0, sprint: true, ..Default::default() };
            run(&mut s, i, &w, 0.15);
            assert!(s.horizontal_speed() > p.sprint_speed - 2.0, "sprint speed comes in at once: {}", s.horizontal_speed());
            assert_eq!(s.sprint_frac, 0.0, "view still waiting for sprintStartDelay");
            run(&mut s, i, &w, 0.05 + p.sprint_start * 0.5);
            assert!((s.sprint_frac - 0.5).abs() < 0.05, "view halfway: {}", s.sprint_frac);
            run(&mut s, i, &w, p.sprint_start);
            assert_eq!(s.sprint_frac, 1.0);
            run(&mut s, PilotInput { forward: 1.0, ..Default::default() }, &w, p.sprint_end + 0.02);
            assert_eq!(s.sprint_frac, 0.0, "out over sprintEndDuration");
            assert!(s.horizontal_speed() < p.walk_speed + 5.0, "back to a walk: {}", s.horizontal_speed());
        }

        #[test]
        fn sprint_and_slide() {
            let w = arena();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput { forward: 1.0, sprint: true, ..Default::default() }, &w, 2.0);
            assert!((s.horizontal_speed() - 260.0).abs() < 0.5, "sprint {}", s.horizontal_speed());
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
            // Walking into it just stops at its face: automantle needs a jump (client.dll
            // 0x180209ed0).
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            run(&mut s, fwd, &w, 3.0);
            assert!(s.on_ground() && s.pos.z.abs() < 1.0 && (s.pos.x - (x0 - 16.0)).abs() < 1.0, "should stop at the crate: {:?} {:?}", s.pos, s.mode);
            // Jumping into it climbs onto it.
            run(&mut s, PilotInput { jump: true, ..fwd }, &w, 1.0 / 120.0);
            run(&mut s, fwd, &w, 2.0);
            assert!((s.pos.z - h).abs() < 2.0 && s.pos.x > x0, "should be on the crate: {:?} {:?}", s.pos, s.mode);
        }

        #[test]
        fn grapple_reels_in_to_the_hook() {
            let w = arena();
            let p = PilotParams::default();
            let look = PilotInput { yaw: std::f32::consts::FRAC_PI_2, pitch: -0.3, ..Default::default() };
            let mut s = PilotState::new(Vec3::new(0.0, -600.0, 0.0), look.yaw);
            run(&mut s, look, &w, 0.2);
            assert!(s.on_ground());
            // Fire at the wall 900 units away: the hook flies at grapple_shootVel (3000), so
            // it lands after about 0.3 s, and the pull waits grapple_pullDelay (0.2 s) more.
            let dir = Vec3::new(0.0, 0.3f32.cos(), 0.3f32.sin());
            assert!(s.fire_grapple(dir, 1100.0, &p));
            let mut ticks = 0;
            while s.grapple.is_none() {
                step(&mut s, &look, &p, &w, 1.0 / 120.0);
                ticks += 1;
                assert!(ticks < 60, "the hook should land");
            }
            assert!((30..=40).contains(&ticks), "flight time {ticks}");
            assert!(matches!(s.grapple_events[0], GrappleEvent::Hooked { .. }));
            let start = s.pos;
            run(&mut s, look, &w, 0.15);
            assert!(!s.grapple_pulling() && (s.pos - start).length() < 1.0, "no pull before the delay");
            let mut arrived = false;
            for _ in 0..(3.0 * 120.0) as usize {
                step(&mut s, &look, &p, &w, 1.0 / 120.0);
                if s.grapple.is_none() {
                    arrived = true;
                    break;
                }
            }
            assert!(arrived, "should reach the hook: {:?} {:?}", s.pos, s.vel);
            assert!(s.grapple_events.contains(&GrappleEvent::Pulling) && s.grapple_events.contains(&GrappleEvent::Released));
            assert!(s.pos.y > 200.0 && s.pos.z > 100.0, "pulled to the wall: {:?}", s.pos);
            assert!(s.vel.z > 0.0, "impact hop: {:?}", s.vel);
            // The released hook comes back to the eye.
            run(&mut s, look, &w, 0.5);
            assert!(s.hook.is_none());

            // Jumping while pulled is a double jump; the grapple holds.
            let mut s = PilotState::new(Vec3::new(0.0, -600.0, 0.0), look.yaw);
            run(&mut s, look, &w, 0.2);
            s.attach_grapple(Vec3::new(0.0, 299.0, 300.0));
            run(&mut s, look, &w, 0.4);
            assert!(s.grapple_pulling());
            step(&mut s, &PilotInput { jump: true, ..look }, &p, &w, 1.0 / 120.0);
            assert!(s.grapple.is_some() && !s.double_jump_available);
            // Turning around lets go.
            step(&mut s, &PilotInput { yaw: -std::f32::consts::FRAC_PI_2, ..look }, &p, &w, 1.0 / 120.0);
            assert!(s.grapple.is_none());
        }

        /// A hook that hits nothing flies its reach and comes back; firing again while it's
        /// stuck lets go.
        #[test]
        fn grapple_misses_and_cancels() {
            let w = arena();
            let p = PilotParams::default();
            let away = PilotInput { yaw: -std::f32::consts::FRAC_PI_2, ..Default::default() };
            let mut s = PilotState::new(Vec3::new(0.0, -600.0, 0.0), away.yaw);
            run(&mut s, away, &w, 0.2);
            assert!(s.fire_grapple(Vec3::new(0.0, -1.0, 0.2), 1100.0, &p));
            run(&mut s, away, &w, 0.4);
            assert_eq!(s.grapple_events, vec![GrappleEvent::Missed]);
            assert!(s.hook.is_some_and(|h| h.back.is_some()));
            run(&mut s, away, &w, 0.3);
            assert!(s.hook.is_none() && s.grapple.is_none());
            s.attach_grapple(Vec3::new(0.0, 299.0, 300.0));
            assert!(!s.fire_grapple(Vec3::Y, 1100.0, &p));
            assert!(s.grapple.is_none() && s.hook.is_some());
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

        /// A slide sooner than slide_boost_cooldown after the last gets no boost; after the
        /// cooldown the full boost is back.
        #[test]
        fn slide_boost_cooldown() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
            let sprint = PilotInput { forward: 1.0, sprint: true, ..Default::default() };
            let slide = PilotInput { forward: 1.0, sprint: true, crouch: true, ..Default::default() };
            run(&mut s, sprint, &w, 2.0);
            let before = s.horizontal_speed();
            step(&mut s, &slide, &p, &w, 1.0 / 120.0);
            let first = s.horizontal_speed() - before;
            let boost = (before + p.slide_boost).min(p.slide_boost_cap) - before;
            assert!((first - boost).abs() < 2.0, "first boost {first}");
            // Hop out of the slide and slide again on landing: no boost this soon.
            step(&mut s, &PilotInput { jump: true, ..slide }, &p, &w, 1.0 / 120.0);
            while !s.on_ground() {
                step(&mut s, &sprint, &p, &w, 1.0 / 120.0);
            }
            let before = s.horizontal_speed();
            step(&mut s, &slide, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Slide);
            assert!(s.horizontal_speed() <= before + 1.0, "no boost inside the cooldown: {before} -> {}", s.horizontal_speed());
            // Rest, then the full boost again.
            run(&mut s, sprint, &w, p.slide_boost_cooldown + 0.5);
            let before = s.horizontal_speed();
            step(&mut s, &slide, &p, &w, 1.0 / 120.0);
            let boost = (before + p.slide_boost).min(p.slide_boost_cap) - before;
            assert!((s.horizontal_speed() - before - boost).abs() < 2.0, "boost recovered");
        }

        /// A slide loses slideDecel per second and keeps slideVelocityDecay of its speed per
        /// second; standing up above slideMaxStopSpeed keeps it going at slideWantToStopDecel.
        #[test]
        fn slide_slows_down() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            s.vel.x = 380.0;
            let crouch = PilotInput { forward: 1.0, crouch: true, ..Default::default() };
            step(&mut s, &crouch, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Slide);
            let v0 = s.horizontal_speed();
            run(&mut s, crouch, &w, 0.5);
            // Two decays over half a second: v -> (v - 50 dt) * 0.7^dt per tick.
            let want = (0..60).fold(v0, |v, _| (v - p.slide_decel / 120.0) * p.slide_velocity_decay.powf(1.0 / 120.0));
            assert!((s.horizontal_speed() - want).abs() < 3.0, "slide speed {} want {want}", s.horizontal_speed());
            // Letting go of crouch while faster than slideMaxStopSpeed keeps sliding.
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            s.vel.x = 500.0;
            step(&mut s, &crouch, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Slide);
            step(&mut s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Slide, "can't stand up above slideMaxStopSpeed");
            assert!(s.crouched, "still crouched while the slide goes on");
            let mut t = 0.0;
            while s.mode == PilotMove::Slide && t < 2.0 {
                step(&mut s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
                t += 1.0 / 120.0;
            }
            // ~150 u/s to lose at slideWantToStopDecel 400 (plus the decay): well under a second.
            assert!(t < 0.45 && s.mode == PilotMove::Ground, "stood up after {t} s at {}", s.horizontal_speed());
            assert!(s.horizontal_speed() <= p.slide_max_stop_speed + 1.0);
        }

        /// A jump out of a slide keeps its speed; once per boosted slide it's a long jump: a speed
        /// within 85% of slideMaxJumpSpeed is raised to it, and jumping before the crouch has
        /// finished takes the boost back.
        #[test]
        fn slide_jump_speeds() {
            let w = arena();
            let p = PilotParams::default();
            let crouch = PilotInput { forward: 1.0, crouch: true, ..Default::default() };
            let slide_at = |v: f32| {
                let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
                run(&mut s, PilotInput::default(), &w, 0.5);
                s.vel.x = v;
                step(&mut s, &crouch, &p, &w, 1.0 / 120.0);
                assert_eq!(s.mode, PilotMove::Slide);
                s
            };
            // Fast slides keep their speed through the jump.
            let mut s = slide_at(480.0);
            run(&mut s, crouch, &w, 0.3);
            let before = s.horizontal_speed();
            step(&mut s, &PilotInput { jump: true, ..crouch }, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Air);
            assert!((s.horizontal_speed() - before).abs() < 1.0, "slide jump speed {} from {before}", s.horizontal_speed());
            // The long jump: within 85% of slideMaxJumpSpeed, raised to it.
            let mut s = slide_at(320.0);
            while s.horizontal_speed() > p.slide_max_jump_speed - 5.0 {
                step(&mut s, &crouch, &p, &w, 1.0 / 120.0);
            }
            step(&mut s, &PilotInput { jump: true, ..crouch }, &p, &w, 1.0 / 120.0);
            assert!((s.horizontal_speed() - p.slide_max_jump_speed).abs() < 0.5, "long jump {}", s.horizontal_speed());
            // Jumping during the crouch-down gives the boost back.
            let mut s = slide_at(300.0);
            let boost = p.slide_boost_cap - 300.0;
            run(&mut s, crouch, &w, 0.05);
            let before = s.horizontal_speed();
            step(&mut s, &PilotInput { jump: true, ..crouch }, &p, &w, 1.0 / 120.0);
            assert!((s.horizontal_speed() - (before - boost)).abs() < 1.0, "early jump {} from {before}", s.horizontal_speed());
        }

        /// Crouching only starts a slide with the movement input within slide_max_angle_dot of
        /// the view's forward, wherever the Pilot is moving.
        #[test]
        fn slide_needs_forward_input() {
            let w = arena();
            let p = PilotParams::default();
            let try_slide = |input: PilotInput| {
                let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
                run(&mut s, PilotInput { forward: 1.0, sprint: true, ..Default::default() }, &w, 2.0);
                step(&mut s, &PilotInput { crouch: true, ..input }, &p, &w, 1.0 / 120.0);
                s.mode == PilotMove::Slide
            };
            assert!(try_slide(PilotInput { forward: 1.0, ..Default::default() }));
            assert!(try_slide(PilotInput { forward: 1.0, right: 1.0, ..Default::default() }), "diagonal");
            assert!(!try_slide(PilotInput::default()), "no input");
            assert!(!try_slide(PilotInput { right: 1.0, ..Default::default() }), "strafe");
            // Looking 70 degrees off the motion while holding forward still slides.
            assert!(try_slide(PilotInput { forward: 1.0, yaw: 70f32.to_radians(), ..Default::default() }));
        }

        /// The view crouches over 0.4 s (0.2 s into a slide) and stands over 0.2 s, eased.
        #[test]
        fn crouch_view_timing() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.3);
            let crouch = PilotInput { crouch: true, ..Default::default() };
            let half = (p.eye_height + p.crouch_eye_height) * 0.5;
            run(&mut s, crouch, &w, 0.2);
            assert!((s.eye(&p).z - half).abs() < 2.0, "halfway down at 0.2 s: {}", s.eye(&p).z);
            run(&mut s, crouch, &w, 0.21);
            assert!((s.eye(&p).z - p.crouch_eye_height).abs() < 0.01);
            run(&mut s, PilotInput::default(), &w, 0.1);
            assert!((s.eye(&p).z - half).abs() < 2.0, "halfway up at 0.1 s: {}", s.eye(&p).z);
            run(&mut s, PilotInput::default(), &w, 0.11);
            assert!((s.eye(&p).z - p.eye_height).abs() < 0.01);
            // Into a slide: 0.2 s.
            run(&mut s, PilotInput { forward: 1.0, sprint: true, ..Default::default() }, &w, 2.0);
            run(&mut s, PilotInput { forward: 1.0, crouch: true, ..Default::default() }, &w, 0.21);
            assert_eq!(s.mode, PilotMove::Slide);
            assert!((s.eye(&p).z - p.crouch_eye_height).abs() < 0.01, "{}", s.eye(&p).z);
        }

        /// A jump within skip_time of landing is a skip: 75% of the height, landing speed kept
        /// (less skip_speed_reduce above skip_speed_retain).
        #[test]
        fn skip_jump() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 30.0), 0.0);
            s.vel.x = 600.0;
            while !s.on_ground() {
                step(&mut s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
            }
            // A few ground ticks of friction, still inside skip_time.
            for _ in 0..6 {
                step(&mut s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
            }
            step(&mut s, &PilotInput { jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            assert!((s.horizontal_speed() - (600.0 - p.skip_speed_reduce)).abs() < 2.0, "skip speed {}", s.horizontal_speed());
            let top = run(&mut s, PilotInput::default(), &w, 0.6);
            assert!((top - p.jump_height * p.skip_jump_frac * p.jump_gravity / p.gravity).abs() < 3.0, "skip apex {top}");
            // Below skip_speed_retain nothing is lost; a later jump is a full one.
            let mut s = PilotState::new(Vec3::new(-3000.0, 0.0, 30.0), 0.0);
            s.vel.x = 300.0;
            while !s.on_ground() {
                step(&mut s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
            }
            step(&mut s, &PilotInput { jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            assert!((s.horizontal_speed() - 300.0).abs() < 2.0, "{}", s.horizontal_speed());
        }

        /// Walking off a ledge leaves jump_graceperiod to still jump normally, keeping the
        /// double jump.
        #[test]
        fn ledge_grace_jump() {
            let mut w = arena();
            // A 200-unit-high platform ending at x = 0.
            quad(&mut w, Vec3::new(-1000.0, -500.0, 200.0), Vec3::new(0.0, -500.0, 200.0), Vec3::new(0.0, 500.0, 200.0), Vec3::new(-1000.0, 500.0, 200.0));
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-100.0, 0.0, 200.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.2);
            assert!(s.on_ground());
            let walk = PilotInput { forward: 1.0, ..Default::default() };
            while s.on_ground() {
                step(&mut s, &walk, &p, &w, 1.0 / 120.0);
            }
            for _ in 0..6 {
                step(&mut s, &walk, &p, &w, 1.0 / 120.0);
            }
            step(&mut s, &PilotInput { jump: true, ..walk }, &p, &w, 1.0 / 120.0);
            assert!(s.double_jump_available, "a grace jump, not the double jump");
            assert!(s.vel.z > 200.0, "jumped: {:?}", s.vel);
        }

        /// A double jump while already rising faster than it would launch still adds
        /// superjump_min_height_fraction of its height.
        #[test]
        fn double_jump_while_rising() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(0.0, -1000.0, 300.0), 0.0);
            s.vel.z = 400.0;
            step(&mut s, &PilotInput { jump: true, ..Default::default() }, &p, &w, 1.0 / 120.0);
            assert!(s.vel.z > 400.0, "kept and added to: {}", s.vel.z);
        }

        /// Landing with crouch held and speed from the air starts a slide without sprinting.
        #[test]
        fn slides_on_landing() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-2000.0, -1000.0, 80.0), 0.0);
            s.vel.x = 350.0;
            let crouch = PilotInput { forward: 1.0, crouch: true, ..Default::default() };
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

        /// Airborne just short of the wall (a wall-run starts on contact).
        fn wallrun_entry(vel: Vec3) -> PilotState {
            let mut s = PilotState::new(Vec3::new(-1500.0, 283.5, 100.0), 0.0);
            s.mode = PilotMove::Air;
            s.vel = vel;
            s
        }

        /// wallrunFriction bleeds entry speed above wallrunMaxSpeedHorizontal.
        #[test]
        fn wallrun_bleeds_excess_speed() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = wallrun_entry(Vec3::new(500.0, 40.0, 50.0));
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            let mut ran = false;
            for _ in 0..60 {
                step(&mut s, &fwd, &p, &w, 1.0 / 120.0);
                ran |= matches!(s.mode, PilotMove::WallRun { .. });
            }
            assert!(ran && matches!(s.mode, PilotMove::WallRun { .. }), "wallrun started: {:?}", s.mode);
            // Friction proportional to the speed against wallrunAccelerateHorizontal: the run
            // levels out where they balance, if that's under wallrunMaxSpeedHorizontal.
            let settle = p.wallrun_max_h.min(p.wallrun_accel_h / p.wallrun.friction);
            assert!((s.horizontal_speed() - settle).abs() < 25.0, "settles at the wallrun speed: {}", s.horizontal_speed());
        }

        /// Starting a wall-run lifts the Pilot (wallrun_upWallBoost), friction and the
        /// gravity ramp level the run out, and wallrun_slipstarttime starts the slip (speed
        /// lost, pushed down) before wallrun_timeLimit drops the Pilot away from the wall.
        #[test]
        fn wallrun_lift_level_and_slip() {
            let w = arena();
            let p = PilotParams::default();
            let wr = &p.wallrun;
            let mut s = wallrun_entry(Vec3::new(340.0, 40.0, 0.0));
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            let z0 = s.pos.z;
            let (mut peak, mut at_1s, mut speed_before_slip, mut speed_after_slip) = (z0, 0.0, 0.0, 0.0);
            let mut last_t = 0.0;
            for _ in 0..240 {
                step(&mut s, &fwd, &p, &w, 1.0 / 120.0);
                peak = peak.max(s.pos.z);
                if let PilotMove::WallRun { time, .. } = s.mode {
                    if time <= 1.0 {
                        at_1s = s.pos.z;
                    }
                    if time < wr.slip_start {
                        speed_before_slip = s.horizontal_speed();
                    } else if speed_after_slip == 0.0 {
                        speed_after_slip = s.horizontal_speed();
                    }
                    last_t = time;
                } else if last_t > 0.0 {
                    break;
                }
            }
            let rise = peak - z0;
            assert!(rise > 20.0 && rise < 120.0, "lift {rise}");
            assert!(at_1s > z0 + 20.0, "still up the wall after a second: {}", at_1s - z0);
            assert!(speed_after_slip < speed_before_slip * 0.6, "slip slowdown {speed_before_slip} -> {speed_after_slip}");
            assert!((last_t - p.wallrun_time).abs() < 0.02, "ran until wallrun_timeLimit: {last_t}");
            assert_eq!(s.mode, PilotMove::Air);
            assert!(s.vel.y < -40.0, "fell away from the wall: {:?}", s.vel);
        }

        /// No input at all slips down the wall (wallrun_noInputSlipFrac).
        #[test]
        fn wallrun_without_input_slips() {
            let w = arena();
            let p = PilotParams::default();
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            let mut held = wallrun_entry(Vec3::new(340.0, 40.0, 0.0));
            let mut idle = wallrun_entry(Vec3::new(340.0, 40.0, 0.0));
            for _ in 0..6 {
                step(&mut held, &fwd, &p, &w, 1.0 / 120.0);
                step(&mut idle, &fwd, &p, &w, 1.0 / 120.0);
            }
            assert!(matches!(idle.mode, PilotMove::WallRun { .. }), "{:?}", idle.mode);
            for _ in 0..60 {
                step(&mut held, &fwd, &p, &w, 1.0 / 120.0);
                step(&mut idle, &PilotInput::default(), &p, &w, 1.0 / 120.0);
            }
            assert!(idle.pos.z < held.pos.z - 15.0, "idle {} held {}", idle.pos.z, held.pos.z);
        }

        /// Pushing away from the wall lets go (wallrun_pushAwayFallOffTime).
        #[test]
        fn wallrun_push_away_falls_off() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = wallrun_entry(Vec3::new(340.0, 40.0, 0.0));
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            for _ in 0..12 {
                step(&mut s, &fwd, &p, &w, 1.0 / 120.0);
            }
            assert!(matches!(s.mode, PilotMove::WallRun { .. }));
            // The wall faces -Y; "right" at yaw 0 is -Y, away from it.
            for _ in 0..10 {
                step(&mut s, &PilotInput { right: 1.0, ..Default::default() }, &p, &w, 1.0 / 120.0);
            }
            assert_eq!(s.mode, PilotMove::Air);
        }

        /// The wall last left can be run again only below where that run started, without the
        /// up-wall boost (wallrun_sameWall*, client.dll 0x180202090); falling off a wall also
        /// locks wall-runs out for 0.5 s.
        #[test]
        fn same_wall_only_lower() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = wallrun_entry(Vec3::new(340.0, 40.0, 0.0));
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            let mut start = None;
            for _ in 0..30 {
                step(&mut s, &fwd, &p, &w, 1.0 / 120.0);
                if matches!(s.mode, PilotMove::WallRun { .. }) {
                    start.get_or_insert(s.pos.z);
                }
            }
            assert!(matches!(s.mode, PilotMove::WallRun { .. }));
            let start = start.unwrap();
            step(&mut s, &PilotInput { jump: true, ..fwd }, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Air);
            // Back at the wall above where the run started: no catch. (No keys: within 0.5 s of
            // the wall jump they would turn the velocity along the view, away from the wall.)
            let back = |s: &mut PilotState, z: f32| {
                s.pos = Vec3::new(s.pos.x, 283.5, z);
                s.vel = Vec3::new(300.0, 40.0, -10.0);
                for _ in 0..3 {
                    step(s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
                }
            };
            back(&mut s, start + 40.0);
            assert_eq!(s.mode, PilotMove::Air, "same wall, higher");
            // Below it: caught, but not boosted up the wall.
            back(&mut s, start - 20.0);
            assert!(matches!(s.mode, PilotMove::WallRun { .. }), "same wall, lower: {:?}", s.mode);
            assert!(s.vel.z < 30.0, "no up-wall boost on the same wall: {:?}", s.vel);
            // Falling off (not jumping): no wall-run for the next half second.
            for _ in 0..3 {
                step(&mut s, &PilotInput { crouch: true, ..fwd }, &p, &w, 1.0 / 120.0);
            }
            assert_eq!(s.mode, PilotMove::Air);
            let low = start - 40.0;
            back(&mut s, low);
            assert_eq!(s.mode, PilotMove::Air, "dropped off: locked out");
            // Half a second later (kept in the air), it may.
            s.pos = Vec3::new(s.pos.x, 0.0, 400.0);
            for _ in 0..60 {
                step(&mut s, &PilotInput::default(), &p, &w, 1.0 / 120.0);
            }
            back(&mut s, low);
            assert!(matches!(s.mode, PilotMove::WallRun { .. }), "lockout over: {:?}", s.mode);
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

        /// Running around the outside of a curved wall (a 600-unit faceted cylinder): the run
        /// turns with the wall (wallrun_rotateMaxRate) instead of running off its tangent.
        #[test]
        fn wallrun_follows_a_curved_wall() {
            let mut w = CollisionWorld::default();
            let s = 4000.0;
            quad(&mut w, Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
            let (c, r) = (Vec3::new(0.0, 900.0, 0.0), 600.0);
            let at = |deg: f32| c + Vec3::new(deg.to_radians().cos(), deg.to_radians().sin(), 0.0) * r;
            let mut deg = -170.0;
            while deg < -10.0 {
                let (a, b) = (at(deg), at(deg + 10.0));
                quad(&mut w, a, a + Vec3::Z * 800.0, b + Vec3::Z * 800.0, b);
                deg += 10.0;
            }
            let p = PilotParams::default();
            let th = (-105.0f32).to_radians();
            let mut st = PilotState::new(c + Vec3::new(th.cos(), th.sin(), 0.0) * (r + 17.0) + Vec3::Z * 100.0, 0.0);
            st.mode = PilotMove::Air;
            st.vel = Vec3::new(-th.sin(), th.cos(), 0.0) * 350.0 - Vec3::new(th.cos(), th.sin(), 0.0) * 40.0 + Vec3::Z * 50.0;
            let mut first = None;
            let mut run_time = 0.0f32;
            for _ in 0..180 {
                // The player steers along the run.
                let yaw = st.vel.y.atan2(st.vel.x);
                step(&mut st, &PilotInput { forward: 1.0, yaw, ..Default::default() }, &p, &w, 1.0 / 120.0);
                if let PilotMove::WallRun { normal, time } = st.mode {
                    first.get_or_insert(normal);
                    run_time = time;
                }
            }
            let PilotMove::WallRun { normal, .. } = st.mode else { panic!("fell off the curved wall after {run_time:.2} s: {:?}", st.mode) };
            let turned = first.unwrap().angle_between(normal);
            assert!(run_time > 1.2 && turned > 0.4, "ran {run_time:.2} s, turned {turned:.2} rad");
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
                // Looking at the wall until on it (the keyboard grace turns the jump along the
                // view), then along it.
                let yaw = if matches!(s.mode, PilotMove::WallRun { .. }) { 0.0 } else { 0.3 };
                step(&mut s, &PilotInput { forward: 1.0, sprint: true, yaw, ..Default::default() }, &p, &w, 1.0 / 120.0);
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
