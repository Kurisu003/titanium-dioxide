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
            assert!((first - p.slide_boost).abs() < 2.0, "first boost {first}");
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
            assert!((s.horizontal_speed() - before - p.slide_boost).abs() < 2.0, "boost recovered");
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
            let crouch = PilotInput { crouch: true, ..Default::default() };
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

        /// Jumping out of a slide is cut to slideMaxJumpSpeed.
        #[test]
        fn slide_jump_speed_cap() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput::default(), &w, 0.5);
            s.vel.x = 480.0;
            let crouch = PilotInput { crouch: true, ..Default::default() };
            step(&mut s, &crouch, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Slide);
            step(&mut s, &PilotInput { jump: true, ..crouch }, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Air);
            assert!((s.horizontal_speed() - p.slide_max_jump_speed).abs() < 1.0, "slide jump speed {}", s.horizontal_speed());
        }

        /// A sprint heading well away from where the view points doesn't become a slide
        /// (slide_max_angle_dot).
        #[test]
        fn slide_needs_heading() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = PilotState::new(Vec3::new(-3000.0, -1000.0, 0.0), 0.0);
            run(&mut s, PilotInput { forward: 1.0, sprint: true, ..Default::default() }, &w, 2.0);
            assert!(s.sprinting);
            // Turn the view 70 degrees away from the motion.
            let yaw = 70f32.to_radians();
            step(&mut s, &PilotInput { forward: 1.0, sprint: true, crouch: true, yaw, ..Default::default() }, &p, &w, 1.0 / 120.0);
            assert_ne!(s.mode, PilotMove::Slide);
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
            assert!((top - p.jump_height * p.skip_jump_frac).abs() < 3.0, "skip apex {top}");
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

        fn wallrun_entry(vel: Vec3) -> PilotState {
            let mut s = PilotState::new(Vec3::new(-1500.0, 280.0, 100.0), 0.0);
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
            assert!((s.horizontal_speed() - p.wallrun_max_h).abs() < 25.0, "settles at the wallrun speed: {}", s.horizontal_speed());
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

        /// After jumping off a wall, the same wall can be run again only no higher than where
        /// the Pilot left it (wallrun_sameWall*).
        #[test]
        fn same_wall_only_lower() {
            let w = arena();
            let p = PilotParams::default();
            let mut s = wallrun_entry(Vec3::new(340.0, 40.0, 0.0));
            let fwd = PilotInput { forward: 1.0, ..Default::default() };
            for _ in 0..30 {
                step(&mut s, &fwd, &p, &w, 1.0 / 120.0);
            }
            assert!(matches!(s.mode, PilotMove::WallRun { .. }));
            let left = s.pos.z;
            step(&mut s, &PilotInput { jump: true, ..fwd }, &p, &w, 1.0 / 120.0);
            assert_eq!(s.mode, PilotMove::Air);
            // Back at the wall above the jump-off point: no catch.
            let back = |s: &mut PilotState, z: f32| {
                s.pos = Vec3::new(s.pos.x, 280.0, z);
                s.vel = Vec3::new(300.0, 40.0, -10.0);
                for _ in 0..3 {
                    step(s, &fwd, &p, &w, 1.0 / 120.0);
                }
            };
            back(&mut s, left + 40.0);
            assert_eq!(s.mode, PilotMove::Air, "same wall, higher");
            // Below it: caught.
            back(&mut s, left - 40.0);
            assert!(matches!(s.mode, PilotMove::WallRun { .. }), "same wall, lower: {:?}", s.mode);
            // Falling off (not jumping) blocks that wall until the ground.
            for _ in 0..3 {
                step(&mut s, &PilotInput { crouch: true, ..fwd }, &p, &w, 1.0 / 120.0);
            }
            assert_eq!(s.mode, PilotMove::Air);
            let below = s.pos.z - 40.0;
            back(&mut s, below);
            assert_eq!(s.mode, PilotMove::Air, "dropped off: blocked");
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
