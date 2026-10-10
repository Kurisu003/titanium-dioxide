//! Slide hopping against client.dll: from a sprint, slide (or jump), land straight into the next
//! slide with crouch held through the air, and jump again a few ticks after each landing.
//! `cargo test -p tf-sim --test slide_hop -- --nocapture` prints the takeoff speed of every hop.
//!
//! The game's rules (StartSlide 0x18020af70, jump handler 0x180200609 / 0x1802009b6): every
//! slide start, on the ground or on landing crouched (SetGroundEntity 0x1801fa1c6), restarts
//! slide_boost_cooldown (2 s), and one that starts after it adds slideSpeedBoost 150 up to
//! slideSpeedBoostCap 400. A jump takes the boost back only while the Pilot isn't fully ducked
//! yet (m_duckState != 2) within 0.4 s of the slide's start: crouch held through the air
//! stays ducked, so landing into a boosted slide and hopping straight away keeps the boost. A
//! skip (a jump within skip_time 0.1 s of landing) keeps the speed it has then, less
//! skip_speed_reduce 12 above skip_speed_retain 450.

use glam::Vec3;
use tf_sim::collision::CollisionWorld;
use tf_sim::pilot::*;

const DT: f32 = 1.0 / 120.0;

fn floor() -> CollisionWorld {
    let mut w = CollisionWorld::default();
    let s = 40000.0;
    let (a, b, c, d) = (Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0));
    w.add(a, b, c);
    w.add(a, c, d);
    w
}

/// Sprints to full speed, then either slides and jumps `first` seconds into the slide
/// (`Some(first)`) or jumps straight out of the sprint (`None`), and hops `hops` times with
/// forward and crouch held, jumping `delay` ticks after each landing. Returns the horizontal
/// speed at each takeoff.
fn slide_hops(p: &PilotParams, w: &CollisionWorld, first: Option<f32>, delay: usize, hops: usize) -> Vec<f32> {
    let mut s = PilotState::new(Vec3::new(-30000.0, 0.0, 0.0), 0.0);
    let sprint = PilotInput { forward: 1.0, sprint: true, ..Default::default() };
    for _ in 0..360 {
        step(&mut s, &sprint, p, w, DT);
    }
    let held = PilotInput { crouch: true, ..sprint };
    let mut takeoff = Vec::new();
    match first {
        Some(first) => {
            for _ in 0..(first / DT).round() as usize {
                step(&mut s, &held, p, w, DT);
            }
            assert_eq!(s.mode, PilotMove::Slide, "the crouch starts a slide");
            step(&mut s, &PilotInput { jump: true, ..held }, p, w, DT);
        }
        None => step(&mut s, &PilotInput { jump: true, ..sprint }, p, w, DT),
    }
    for hop in 0..hops {
        assert_eq!(s.mode, PilotMove::Air);
        takeoff.push(s.horizontal_speed());
        let mut t = 0;
        while !s.on_ground() && t < 600 {
            step(&mut s, &held, p, w, DT);
            t += 1;
        }
        for _ in 0..delay {
            step(&mut s, &held, p, w, DT);
            assert_eq!(s.mode, PilotMove::Slide, "hop {hop}: landing crouched slides");
        }
        step(&mut s, &PilotInput { jump: true, ..held }, p, w, DT);
    }
    takeoff
}

fn row(v: &[f32]) -> String {
    v.iter().map(|x| format!("{x:4.0}")).collect::<Vec<_>>().join(" ")
}

#[test]
fn slide_hop_speeds() {
    let (p, w) = (PilotParams::default(), floor());
    let hops = 8;
    // Slide out of the sprint (260 + 150, capped at 400), jump once the crouch is done.
    let slide = slide_hops(&p, &w, Some(0.3), 3, hops);
    // Jump out of the sprint, crouch in the air, land into the (boosted) slide and hop on.
    let jump = slide_hops(&p, &w, None, 3, hops);
    // Jump within 0.2 s of the slide start (still ducking): the boost is taken back.
    let early = slide_hops(&p, &w, Some(0.1), 3, hops);
    println!("takeoff speeds, jumping 3 ticks (25 ms) after each landing:");
    println!("  slide 0.3 s, then hop:     {}", row(&slide));
    println!("  sprint jump, land sliding: {}", row(&jump));
    println!("  slide 0.1 s, then hop:     {}", row(&early));

    // The slide's long jump raises 85-100% of slideMaxJumpSpeed to it.
    assert!((slide[0] - p.slide_max_jump_speed).abs() < 0.5, "long jump: {}", slide[0]);
    // Landing crouched into a slide 2 s after the last one adds the boost (260 + 150, capped
    // at 400), and hopping straight out of it keeps it.
    assert!((jump[0] - p.sprint_speed).abs() < 1.0, "sprint jump: {}", jump[0]);
    assert!(jump[1] > 390.0 && jump[1] < p.slide_boost_cap, "landing slide boost kept: {}", jump[1]);
    // Jumping while still ducking into a fresh slide takes the boost back.
    assert!(early[0] < p.sprint_speed, "early jump refund: {}", early[0]);
    // Further landings come within the cooldown of the last slide: no boost, and each hop
    // loses only the slide friction of its ground ticks (no skip speed loss under 450).
    for v in [&slide, &jump, &early] {
        for w in v[1..].windows(2) {
            let loss = w[0] - w[1];
            assert!(loss > 1.0 && loss < 8.0, "per-hop loss {loss} in {}", row(v));
        }
    }
}

/// Sprint, slide briefly, sprint again: the boost needs slide_boost_cooldown since the last
/// slide's start, boosted or not (StartSlide 0x18020b0b9 / 0x18020b213).
#[test]
fn slide_boost_cooldown() {
    let (p, w) = (PilotParams::default(), floor());
    let mut s = PilotState::new(Vec3::new(-30000.0, 0.0, 0.0), 0.0);
    let sprint = PilotInput { forward: 1.0, sprint: true, ..Default::default() };
    let crouch = PilotInput { crouch: true, ..sprint };
    let mut boosts = Vec::new();
    // Slides 0, 1.5, 3.0 and 5.5 s after a 2 s sprint (each held 0.1 s, then sprinting again).
    let starts = [0.0, 1.5, 3.0, 5.5];
    let mut t = 0.0;
    for at in starts {
        while t < 2.0 + at {
            step(&mut s, &sprint, &p, &w, DT);
            t += DT;
        }
        let before = s.horizontal_speed();
        step(&mut s, &crouch, &p, &w, DT);
        t += DT;
        assert_eq!(s.mode, PilotMove::Slide);
        boosts.push(s.horizontal_speed() - before);
        for _ in 0..12 {
            step(&mut s, &crouch, &p, &w, DT);
            t += DT;
        }
    }
    println!("boosts at {starts:?} s: {boosts:?}");
    assert!(boosts[0] > 100.0, "first slide boosted");
    assert!(boosts[1] < 1.0, "1.5 s later: no boost");
    assert!(boosts[2] < 1.0, "1.5 s after an unboosted slide: still none");
    assert!(boosts[3] > 100.0, "2.5 s later: boosted again");
}
