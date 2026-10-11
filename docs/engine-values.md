# Engine values from the decompiled client.dll (2026-10-11)

What the game's own code says about the Pilot movement rules that were still marked "unverified"
in `handover.md`. Read from a Ghidra decompile of `bin/x64_retail/client.dll` (build v2.0.11.0).
The decompile itself, the full ConVar table (2108 registrations with defaults and help) and the
player-settings table (152 keys with struct offset, default and help) live outside the repo in
`~/tools/ghidra-proj/out` (see the memory note "Titanfall Ghidra decompile"). Nothing from the
binary goes in the repo; only the rules and numbers below.

Addresses are client.dll image addresses. `settings` is the player-settings struct (`.set` values
over the defaults in the table); `mv` is the move data.

## Duck (`0x1801f5500`) and the lost jump

- `m_duckState` (player `+0x245c`): 0 standing, 1 ducking, 2 ducked, 3 unducking.
  `m_flDucktime` (`+0x188c`) is an integer in milliseconds.
- Each player move frame first does `m_flDucktime -= (int)(frametime * 1000)` (truncated per frame,
  `0x1802041c0`), then calls Duck, then the move type's step. Duck runs before the jump check in
  the same frame.
- Pressing crouch from standing: state 1, time 400 ms (200 ms when the crouch starts a slide).
  When the time runs out (`< 1`), FinishDuck (`0x1801f7290`) sets state 2.
- Letting go while ducked (state 2): state 3, time 200 ms. Letting go while still ducking
  (state 1): state 3, time `(1 - duckFraction) * 200` ms. FinishUnDuck (`0x1801f75e0`) sets state 0.
  Letting go only starts the unduck if the standing hull fits (`0x1801f2cd0`); otherwise the state
  goes back to 2.
- Pressing crouch again while unducking (state 3): state 1, time `unduckFraction * (400 or 200)`.
- While sliding (`+0x2600`) with speed above `slideMaxStopSpeed` (350), a released crouch is
  ignored: the Pilot stays ducked and sliding. Below it the slide ends the moment the unduck starts
  (`EndSlide 0x1801f5a20`, called from Duck when the state leaves 1/2).
- The jump check (`0x180200200`) returns at once when `m_duckState == 3` and the Pilot is not
  sliding. It needs a jump press *this frame* (`mv+0x20` is the pressed-this-frame button mask; the
  held mask is `mv+0x18`). Holding jump never repeats a jump, and a press during the unduck is
  dropped, not queued.

So the engine really does lose a jump pressed within 200 ms of standing up out of a slow slide
(speed at or below 350). The sim models exactly this. A jump from a slide that is still above
350 works, and the jump itself ends the slide. If the user still feels jumps being eaten, the
difference is in how fast our slide drops under 350, not in the duck rule.

## Jump (`0x180200200`)

- Height: `settings.slideJumpHeight` (50) only while sliding; otherwise `settings.jumpHeight`
  (60 in `pilot_mp.set`) whatever the duck state. A crouched, non-sliding jump is a full jump.
- Jump speed `sqrt(2 * sv_gravity * height)` is added to the velocity along the ground normal
  (`+0x2494`), scaled by a status multiplier (`+0x2f48` object, 1.0 normally), and by
  `grapple_jumpFrac` (1.0) when grappled.
- Slide boost removal on a jump (`+0x2601` boost pending): once per boosted slide, if the Pilot is
  not fully ducked (`m_duckState != 2`) and the slide started less than 0.4 s ago, the boost
  amount (`+0x2608`) is taken off the speed. Then a speed between `0.85 * slideMaxJumpSpeed` and
  `slideMaxJumpSpeed` (350) is raised to exactly 350.
- Skip (jump within `skip_time` 0.1 s of landing): height × `skip_jump_height_fraction` 0.75;
  speed above `skip_speed_retain` 450 loses `skip_speed_reduce` 12, not below 450.
- If the wish direction points into the last blocking normal (`+0x2b70`), 30 u/s along that
  normal is removed.

## Double jump (superjump branch of `0x180200200`)

- Height: lerp(`superjumpMinHeight`, `superjumpMaxHeight`) by the jump-power fraction
  (`0x1802c7720`); both 60 in `pilot_mp.set` (defaults 80/80). `superjumpLimit` 1 per flight.
- Vertical: with `rising = max(vz, 0)^2 / (2 g)` and `minFrac = superjump_min_height_fraction`
  (0.25): if `height * minFrac + rising < height`, vz is zeroed and the full height is added;
  otherwise `height * minFrac` is added on top of the current vz.
- Horizontal: `add = wishdir * superjumpHorzSpeed` (180 in `pilot_mp.set`, default 240). The old
  horizontal velocity is scaled by `1 - |add| / |v|` (0 if `|add| >= |v|`) and `add` is added.
  This is what the sim does already (`superjump()` in `tf-sim/src/pilot.rs`).

## Wall-run jump (`0x180201170`) — differs from the sim

The sim does `vel = (h + normal * 205 + wish * 80, 230)`. The game does:

1. `add = wishdir * wallrunJumpInputDirSpeed` (80). When the "fill input" flag (`+0x2588`) is set
   and `|wishdir| < 1`, the missing part is filled with the view forward vector first.
2. `outward = wallrunJumpOutwardSpeed` (205) × `(1 - 0.8 * a * b)` where `a = min(0, normal·view)`
   and `b = min(0, normal·wishdir)`: looking and pushing into the wall reduces the push-off.
3. If `normal·add < outward`, `add += normal * (outward - normal·add)`, so the push-off is the
   normal component of the whole `add`, not 205 on top of the input term.
4. `vel += add` (the horizontal velocity is kept, as in the sim).
5. Vertical: `up = wallrunJumpUpSpeed` (230) × strength (1.0 until `wallrun_strengthLossStart`
   1000 wall runs). If `vz < up`: `vz = min(vz + 1.5 * up, up)`. A Pilot falling fast off a wall
   gets a weaker jump; a Pilot rising faster than 230 keeps its speed.

## Values confirmed from the data files (VPK `scripts/players/mp/pilot_mp.set`, `pilot_base.set`)

`jumpheight 60`, `gravityscale 0.80`, `stepheight 18`, `slidedecel 50`, `slidevelocitydecay 0.7`,
`superjumpMinHeight 60`, `superjumpMaxHeight 60`, `superjumpHorzSpeed 180`,
`wallrunJumpUpSpeed 230`, `wallrunJumpOutwardSpeed 205`, `wallrunJumpInputDirSpeed 80`,
`wallrunMaxSpeedHorizontal 420`, `wallrunMaxSpeedVertical 225`, `wallrunAccelerateHorizontal 1500`,
`wallrunAccelerateVertical 360`, `wallrun_timeLimit 1.75`, `wallrun_hangTimeLimit 4`. These match
`PilotParams::default()`.

Player-settings defaults used where the `.set` files are silent: `slideAccel 50`,
`slideRequiredStartSpeed 200`, `slideStopSpeed 125`, `slideMaxStopSpeed 350`,
`slideMaxJumpSpeed 350`, `slideSpeedBoost 150`, `slideSpeedBoostCap 400`, `slideJumpHeight 50`,
`dodgeHeight 25`, `dodgeInterval 0.6`.

ConVar defaults (client.dll): `sv_gravity 750`, `sv_stopspeed 100`, `sv_friction 4`,
`sv_airaccelerate 10`, `jump_graceperiod 0.2`, `jump_keyboardgraceperiodmin 0.2` / `max 0.5`,
`jump_keyboardgrace_strength 0.7`, `jump_keyboardgrace_max 0.7`, `skip_time 0.1`,
`skip_jump_height_fraction 0.75`, `skip_speed_reduce 12`, `skip_speed_retain 450`,
`skip_replenish_double_jump 1`, `superjump_min_height_fraction 0.25`, `slide_boost_cooldown 2`,
`slide_max_angle_dot 0.6`, `slide_step_velocity_reduction 10`, `slide_auto_stand 0`,
`slide_whileInAir 0`, `wallrun_sameWallDist 100`, `wallrun_sameWallSlope 0`,
`wallrun_strengthLossStart 1000`, `wallrun_strengthLossEnd 1000`, `grapple_jumpFrac 1.0`.
