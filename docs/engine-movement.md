# Pilot movement as the engine runs it (client.dll, build v2.0.11.0)

The per-frame movement pipeline of Titanfall 2's prediction code, read from a Ghidra decompile
of `bin/x64_retail/client.dll` (2026-10-14; the same build as `engine-values.md`, which holds the
Duck, jump, double-jump and wall-run jump rules and the confirmed numbers). The decompile and the
tooling live outside the repo (`~/tools/ghidra-proj/out`, `~/tools/rea-*`); only rules and numbers
are written down here. The server runs the same code in `server.dll`.

Names are ours; the binary has none. `mv` is the move data (CMoveData: `+0x18` held buttons,
`+0x1c` old buttons, `+0x20` pressed this frame, `+0x24/+0x28/+0x2c` forward/side/up move,
`+0x6c` wish direction, `+0x88` max speed, `+0x8c` velocity, `+0xb0` summed wish, `+0xd8` origin),
`player` the C_Player, `settings` the player-settings struct (`.set` values over the defaults),
`pose` the current pose block (`stand` / `crouch` ... of the `.set`, picked by GetPose
`0x1802c5ac0` and looked up by `0x18019ed50`). `dt` is the frame time.

## 1. PlayerMove (`0x1802041c0`), once per client frame

1. `settings` and `pose` pointers are refreshed from the `.set` data.
2. If the player is frozen (`player+0x6c8`) the move inputs are zeroed.
3. `m_flDucktime` (`+0x188c`, **an integer in milliseconds**) loses `(int)(dt * 1000)` and stops
   at 0. The cast truncates every frame: at 130 fps each frame takes 7 ms off, so the 200 ms unduck
   lasts 29 frames (223 ms) and the 400 ms duck 58 frames (446 ms); at 60 fps 13 frames (217 ms)
   and 25 frames (417 ms).
4. If the player is carried by a parent entity (handle `+0xf8`) or embarked: the slide is ended,
   Duck runs, and the frame finishes with no move.
5. Movement-type bookkeeping (rodeo, zipline, traversal), the dodge timer, then Duck
   (`0x1801f5500`), the sprint ramp (`0x180203de0`), the grapple state (`0x1801fecc0` ->
   `player+0x25f8`), and `mv.maxspeed = GetMaxSpeed (0x1802c6920) x status multiplier
   (0x1801fcb50)`. Sprint tilt (`0x18020c950`).
6. Dispatch on the move type (`0x1800c1c60`):
   - 0 none: velocity zeroed.
   - 1, and 2 / 7 when not on a zipline and not mantling: **FullWalkMove** (`0x1801f93c0`), the
     Pilot's normal move.
   - 2 / 7 on a zipline: `0x18020f040`; 2 / 7 with a mantle available (`0x18020b2d0`) and 12:
     TraversalMove / automantle (`0x180208bf0`).
   - 4 / 5: fly / toss move (`0x1801f8d60`). 9: ladder (`0x1801f84b0`, sv_ladder cvars).
     10: noclip / spectator (`0x1801f8940`). 13: rodeo (`0x180564650`). 14: operator
     (`0x1804fd2a0`). 15: melee lunge (`0x1802028c0`).
7. After the move: `0x1801fee10` (the sim models it), the velocity is kept for the next frame
   (`mv+0x98`), landing sounds and the step counter.

## 2. FullWalkMove (`0x1801f93c0`)

In order, every frame on move type 1 / 2 / 7:

1. The dodge state (`0x1801f53e0`: flags `+0x2bad` / `+0x2b7c`, `dodgeStopSpeed`,
   `dodgeInterval`).
2. **StartGravity**: `vel += gravity_dir * (gravityscale x sv_gravity x 0.5 x dt x timescale x ramp)`.
   `gravityscale` comes from the player (`vtable+0x5d8`, 0.8 for Pilots), `ramp` is the wall-run
   gravity ramp (`0x18020e6f0` over the time since the wall-run started) and the grapple's gravity
   fraction (`0x1801fe9e0`). **While sliding on the ground the same half tick of gravity is also
   added along the floor's downhill direction** (`vel.xy += g_half * n.xy * n.z`, `n` the ground
   normal): a slide is pulled down slopes. FinishGravity (`0x1801f7410`, step 12) does the same.
3. CheckVelocity (`0x1801f4370`, `sv_maxvelocity`).
4. `0x1802039a0`: only while carried by a Titan / carrying one, caps the velocity.
5. **CheckJumpButton** (`0x180200200`, see `engine-values.md`). Two rules not noted there: with
   crouch toggled on, a jump press only clears the toggle and does not jump when on the ground and
   not sliding (`0x1802002d2`); the press must be in this frame's pressed mask (`mv+0x20 & 2`,
   or its bit 28 for the alternate jump input).
6. The wall-run update (`0x18020e7e0`; when not wall-running and in the air it looks ahead
   `wallrun_lookahead` seconds for a wall through `0x180202090`).
7. Grapple (`0x1801fd7a0`), tether (`0x180207e00`), gravity-grenade pull (`0x1801ff7e0`).
8. CheckVelocity again.
9. On the ground (`0x1802a4680` != 0): **WalkMove** (`0x18020cc30`), else AirMove
   (`0x1801f1bd0`; wall-running first runs `0x1802c2e80`).
10. The impact check (`0x1801f45a0`): the loss of speed perpendicular to gravity during the move,
    against `settings.impactSpeed` (380 for Pilots): more than it is a full impact, more than half
    of it an impact at 0.85 (`0x180205040`, sound and anim).
11. CategorizePosition (`0x1801f3160`).
12. FinishGravity (`0x1801f7410`): the second half tick, with the same slide slope pull.
13. On the ground the velocity along the ground normal is removed.
14. CheckVelocity, the heavy-landing event (`0x1801f40b0`, "heavyland"), `0x1801f4870`.

## 3. WalkMove (`0x18020cc30`), the ground move

1. The velocity along the ground normal (`player+0x2494`) is removed; `+0x18a4` (fall speed) is
   zeroed.
2. Forward / right from the view angles, projected onto the ground plane (when wall-running with
   the right vector nearly along the wall, the other way round).
3. Not wall-running: `fmove = mv+0x24`, `smove = mv+0x28`, divided by `0x1802c7600` when
   `player+0x2434` is set. While ducking or ducked (`m_duckState` 1 or 2) the input length is
   capped at `GetMaxSpeed(pose 1) / GetMaxSpeed(pose 0)` (crouch speed over stand speed), and an
   analog input shorter than that is scaled by `(2 - len) x ratio`; a full keyboard input is
   unchanged. Then `smove *= settings.speedScaleSide` (1), a negative `fmove *=
   settings.speedScaleBack` (1). Wall-running uses the wall normal instead
   (`DAT_1811ae508`, a wall-run input cvar).
4. `wishvel = fwd * fmove + right * smove`. Standing on an entity whose `vtable+0x4a8` says yes and
   `+0x4b0` says no (and the player's `+0x4c8` no): `wishvel = wishvel * 0.25 + 150 * dir_to_it`.
5. The wish is flattened onto the ground plane, then (not wall-running):
   - `RepelFromOtherPlayers` (`0x1802066c0`): 20 x dt x a repel vector away from overlapping
     players, and `0x1802064d0` when `0x18026d9f0` is set.
   - **While sliding** the part of the wish along the velocity is dropped when it points along it
     (always when the forced-slide state `player+0x18b8 == 3`): the input can only steer across a
     slide or brake against it.
   - `wishspeed = min(|wish|, 1) x mv.maxspeed`, `wishdir = wish / |wish|`.
   - `accel = GetAcceleration (0x1801fcbd0)`, **Friction** (`0x1801f8090`), **Accelerate**
     (`0x1801f0ac0`), and `mv+0xb0 += wishdir * wishspeed`.
   Wall-running instead: friction `settings.wallrunFriction` (4) x `player+0x24c4` x `+0x2f50` x the
   impulse ramp (below), 10 when `settings+0x218` is set, Source-style (`speed - speed x dt x f`),
   then vertical and horizontal wall-run acceleration (`wallrunAccelerateVertical` /
   `Horizontal`, backward at `wallrunMaxSpeedHorizontalBackward`).
6. If the velocity before the move was under 1 u/s and is now above: `player+0x2420 = now + 0.4`.
7. **A slide ends** (`EndSlide 0x1801f5a20`) when the speed is under `slideStopSpeed` (125) and
   the forced-slide state is not 3.
8. Under 1 u/s the velocity is zeroed and the move ends. Otherwise `0x18026e4e0` pushes away from
   nearby players (`sv_pushaway_player_*`), the velocity is turned onto the ground plane keeping
   its length (unless the ground entity's `vtable[3]+0x88` says no), the destination
   `origin + vel x dt` is kept off the floor plane, traced (`0x1801fbdc0`), and either taken
   (fraction 1) or run through the blocked handler (`0x1802055b0`) and StepMove (`0x18020b390`),
   with the blocking normal stored in `player+0x2b70`. `0x180207860` finishes (stay on ground).

## 4. Friction (`0x1801f8090`) and GetDeceleration (`0x1801fcf10`)

Friction gets `wishdir` and `wishspeed`. It is not Source friction: it is a linear deceleration of
the velocity that is not along the input.

1. `along = vel . wishdir`. If `along <= 0` nothing is protected. Otherwise
   `k = min(1, along x wishspeed / |vel|^2)` and `keep = wishdir x along x k` (when the wish points
   into the last blocking normal `player+0x2b70`, the wish turned onto that surface plus 1/128 of
   its normal is used instead). So moving along the input at or under the wish speed is not
   braked at all; above it, only the excess (and anything sideways) is.
2. `rest = vel - keep`, `decel = GetDeceleration x player+0x2f50 x player+0x24c8`,
   `|rest|' = max(0, |rest| - decel x dt)`.
3. **Sliding**: `|rest|' *= slideVelocityDecay ^ ((1 - 0.75 x stim) x dt)` with `stim` the speed
   status effect (`0x180328bf0`, 0 normally). `slideVelocityDecay` is 0.7 (`pilot_base.set`).
4. The velocity becomes `rest x (|rest|'/|rest|) + keep`, and `mv+0xb0` is reduced by the same
   fraction of `rest`.

GetDeceleration:

- Not sliding: faster than `GetMaxSpeed(pose)` -> `pose.sprintDeceleration`, or
  `pose.sprintAcceleration` when that is -1. When neither is set (or not faster):
  `pose.deceleration`, and when that is -1 too `0.6 x pose.acceleration`. Pilots set none of
  these: acceleration 2500 -> deceleration 1500 at any speed.
- Sliding: forced-slide state 3 -> 0 (no deceleration). Crouch not held and not toggled, or the
  wish pointing against the velocity -> `slideWantToStopDecel` (400). Else `slideDecel` (50 in
  `pilot_base.set`; the table default is 160).
- The result is multiplied by the **impulse ramp**: `player+0x2450` is a time set on a knockback
  (`0x1802cc3c0`: `now + |impulse| x impulse_low_decel_duration_scalar (0.003)`); until half a
  second before it the factor is 0, then it ramps to 1. It is in the past in normal play.
- And by `player+0x18c0` (a movement scale, 1).

`player+0x2f50` is 1 and is set to 0.25 when CategorizePosition finds the Pilot standing on an
entity it cannot stand on (`vtable+0x308`), recovering x1.25 per frame (`0x1801f2fd0`).

## 5. Accelerate (`0x1801f0ac0`) and GetAcceleration (`0x1801fcbd0`)

Accelerate gets `wishdir`, `wishspeed`, `accel`. Skipped while `player+0x1e98` or `+0x2864`.

1. `cap^2 = max(|vel|^2, wishspeed^2)`.
2. `add = wishspeed - vel . wishdir`; if `add > 0`: `step = min(add, accel x dt x player+0x2f50)`,
   `vel += wishdir x step`, and if `|vel| > cap` the velocity is scaled back to `cap`.

So `accel` is in u/s^2 and does not scale with the wish speed as Source's does.

GetAcceleration (x `player+0x18c0`):

- Sliding: `slideAccel` (50).
- Otherwise, with `pose.sprintAcceleration` set (>= 0) and `|vel|^2 > (GetMaxSpeed(pose) - 1)^2`:
  `sprintAcceleration`. Else if `|vel| >= lowSpeed-speed (0x1802c6fc0)` or `pose.lowAcceleration`
  is -1: `pose.acceleration` (2500); else `pose.lowAcceleration`.

The pose block keys (`.set` per pose) are `speed`, `sprintspeed`, `lowSpeed`, `acceleration`,
`deceleration`, `lowAcceleration`, `sprintAcceleration`, `sprintDeceleration`, `viewheight`,
`firstpersonproxyoffset`, `hull_min`, `hull_max` and the footstep keys. `pilot_mp.set` sets only
`speed 173.5 / sprintspeed 260` for `stand` (250 / 375 in the coliseum pose mod) and `speed 80` for
`crouch`.

## 6. StartSlide (`0x18020af70`) and EndSlide (`0x1801f5a20`)

StartSlide gets the floor plane (needs `normal.z >= 0.7`, or `slide_whileInAir`). In the
forced-slide state 3 it always starts (speed floor 0.1). Otherwise `settings.slide` must be on,
the horizontal speed at least `slideRequiredStartSpeed` (200), and on the ground the wish must be
within `slide_max_angle_dot` (0.6) of the view forward. Then `player+0x2600 = 1`, the slide sounds
and effects, and if the last slide started more than `slide_boost_cooldown` (2 s) ago
(`player+0x2604`): `target = min(speed + slideSpeedBoost 150, slideSpeedBoostCap 400)`; if
`target > speed` the horizontal velocity is scaled to `target` and `player+0x2608 = target - speed`
(the amount CheckJumpButton can take back), else the amount is 0; `+0x2601` (boost pending) is
set. `+0x2604 = now` in every case. Duck calls it when crouch goes down on the ground; OnLand
calls it when landing while ducking or ducked (`m_duckState` 1 or 2).

EndSlide clears `+0x2600`, runs the slide-end effects, and with `slide_auto_stand` clears the
duck toggle. WalkMove calls it under `slideStopSpeed`; Duck calls it when the duck state leaves
1 / 2 (the unduck starting), which a release above `slideMaxStopSpeed` (350) cannot do.

## 7. OnLand (`0x1801fa160`)

Called with the landing trace when the ground entity changes. Landing from the air stores the
landing time (`player+0x2b98`, used by the skip rules), starts a slide when ducking or ducked, and
sets the ground entity. Then, if `skip_replenish_double_jump` is on or the landing was more than
`skip_time` ago or the Pilot is wall-running, the double jump is replenished (`0x1802ce340`); the
ground entity gets the impact; and, not on a ladder, the velocity along the ground normal is
removed.

## 8. CategorizePosition (`0x1801f3160`), the notable parts

- `player+0x2f50 = 1` at the start. Standing on an entity that cannot be stood on
  (`vtable+0x308`): OnLand, the velocity along its normal is removed, and if the horizontal speed
  is under 100 (200 for the larger class) a push-off of 400 (500) u/s away from it is stored in
  `player+0x3d44`; `+0x2f50 = 0.25` so Friction and Accelerate are quartered until it recovers.
- A ground trace from 2 units above (`0x1802081b0` with hull 7); a slope under 0.7 is not ground
  unless wall-running, and leaving the ground records `player+0x2b6c` (the time, for the
  jump grace).
- Wall-running on a surface between -0.35 and 0.7: the stored ground normal is turned toward the
  new one by at most a cvar angle per frame (`0x1802068c0`).

## 9. Where our sim differs or leaves things out (2026-10-14)

The sim already models the pipeline above (its comments cite the same functions). What it does
not do, in order of how likely a player feels it:

1. The duck timer is float seconds, so the unduck and the duck finish 200 / 400 ms after they
   start; the engine's integer-millisecond timer with per-frame truncation makes them 223 / 446 ms
   at 130 fps. A jump pressed 200 to 223 ms after letting go of crouch works in the sim and is
   dropped in the game; the slide boost's 0.4 s take-back window and the "fully ducked" tests
   shift the same way.
2. Slides are not pulled downhill (and uphill slides are not braked) by the slope part of
   gravity. On the flat capture spot this does nothing; on Kodai's ramps it does.
3. The impulse ramp (no deceleration after a knockback), the forced-slide state, standing on an
   entity that cannot be stood on, the 0.25 friction / accel factor, analog input scaling and the
   entity-centre pull in WalkMove are not modeled; none of them act in ordinary running, sliding
   and jumping.

Everything the sim does model agrees with the code read here: the slide deceleration (50, or 400
when the crouch is let go or the input brakes) and the 0.7 per-second decay, the slide speed
boost and its cap, the 350 u/s stop and jump rules, the half-tick gravity, the protected-component
friction and the linear deceleration, the acceleration cap, and the landing slide.
