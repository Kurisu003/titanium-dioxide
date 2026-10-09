# Fix plan (from play-test feedback, 2026-10-07)

This is the list of everything found in the first hands-on play test. For each problem it gives what is
probably wrong, how to find out how Titanfall 2 really does it, how to fix it, and how to check the fix.
Nothing here has been fixed yet.

## Ground rules

1. **Look at the reference first, the code second.** Every fix starts by showing how the real game
   behaves (a number from its data, or a measurement from footage) and how ours differs. No more tuning by eye.
2. **Fix one thing at a time.** Each fix gets its own before/after comparison and its own commit.
3. **Don't override good data with guesses.** If a value has to be guessed, it is labelled as a guess in the code and in the README.
4. **Every fix must be repeatable.** Each one gets a `--script`/`--shots` test, so the comparison can be re-run later.

## How we research (shared toolkit)

These methods apply to all the items below. Each item says which ones it needs.

| ID | Method | What it gives | Needs the user? |
|----|--------|---------------|-----------------|
| **R1** | **Engine defaults from the game binaries.** Read the ConVar registration tables in `client.dll`, `server.dll` and `engine.dll` (name, default value, help text, min and max). Today `strings` and the README; later a small parser in `tools/`. | The real numbers behind the "engine defaults" that are guessed today: cockpit sway, viewmodel bob, wallrun, slide, grapple, titan dash, camera shake, and more. | No |
| **R2** | **Data files we don't read yet.** Check `.set` player settings, weapon `.txt` keys, `springs.txt`, RUI assets and `.vmt` material flags, keeping a list of the keys we ignore. | Which keys we parse wrongly or not at all. | No |
| **R3** | **Reference footage.** Find 60 fps gameplay clips (YouTube, speedrun and movement-tech videos), step through them frame by frame, and measure on-screen values (HUD offset in pixels, how long a motion takes, speed read from `cl_showpos` in movement videos). | How the real game looks and moves, item by item. | No (optional clips, see R4) |
| **R4** | **Our own captures from the real game.** TF2 is installed at `/mnt/4TB/SteamLibrary/.../Titanfall2`. The user plays short, scripted actions (turn at a fixed speed, sprint, slide, ADS, dash) and records them. With an offline private match through the Northstar client there is also a console: `cl_showpos`, `cvarlist`, `host_timescale 0.25`, and `noclip` to reach spots. | Exact side-by-side material for the same action, plus live ConVar values. | **Yes**: the user records, or approves installing Northstar |
| **R5** | **Side-by-side harness.** Run the same action script in our build, then put frame strips next to the reference frames. Add a telemetry dump (`TF_TRACE=…`) that writes per-frame speed, movement state, cooldowns and sway offsets to CSV, so curves can be compared and not just pictures. | Proof that a fix brought us closer to the game. | No |
| **R6** | **RenderDoc captures** of our frames (and of the real game through DXVK/Vulkan, if R4 is possible). | Draw order, blend states, depth tests and shader inputs, for every rendering problem. | Optional |
| **R7** | **Kill switches.** Add an env switch for each made-up motion term (`TF_NO_COCKPIT_SWAY`, `TF_NO_VM_BOB`, `TF_NO_LAND_DIP`, …), so each one can be turned off and compared with the reference on its own. | Finding out which term causes the shaking. | No |

## The issues

Each item is tagged with its group and priority: **A** = concrete bug, probably quick; **B** = feel, needs
measurement; **C** = visuals and rendering.

### Feel and motion

**1. Titan HUD moves far too much** (B, high)
- *Likely cause:* the Titan HUD rides BT's cockpit sway (`vmmotion::CockpitSway`, `CockpitSwayDef::TITAN`, `player.rs` ~996). Those "engine defaults" (gain 3, turn −4, origin −1) and the scaling constants `TURN_K`/`MOVE_K` are reconstructions, not read values. The footstep jolt (`step_jolt`) and the damage jolt add to it.
- *Research:* R1 for `cockpitSway*`, `cockpit_spring*` and `cockpit_stepJolt*`. R3/R4 to measure how many pixels the HUD moves at a known turn speed. Also check whether the HUD RUI is really attached to the cockpit model or to a damped copy of it.
- *Fix:* use the real defaults and rebuild the scaling to match the measurement. If the HUD only follows the cockpit partly, model that.
- *Verify:* R5 turn and strafe scripts, comparing HUD offset curves with the reference.

**2. In general everything moves too much and is too shaky** (B, high)
- *Likely cause:* several made-up motion terms add together:
  - the camera shake noise (`pilotctl.rs` ~934: sine jitter × 6 units);
  - the landing dip spring (`pilotctl.rs` ~830, constants 180/22);
  - sprint tilt;
  - viewmodel sway and bob (`vmmotion.rs`, the combination is reconstructed);
  - the pilot HUD sway (`hud_sway`, `CockpitSwayDef::PILOT`);
  - Titan footstep jolts.
- *Research:* R7 to switch each term off in turn. R1/R2 for the real keys (`bob_*`, `sway_*`, `viewPunch`, `landing*`, `cl_bob*`). R3/R4 standing, walking, sprinting and landing clips.
- *Fix:* keep only the terms the game really has, with its numbers. Remove the ones we invented (likely the procedural camera jitter).
- *Verify:* R5 with identical walk/sprint/land scripts.

**3. ADS animation is weird** (B, high)
- *Likely cause:* to be determined. Candidates:
  - the blend from hip to ADS pose (timing curve, `zoom_time_in`);
  - sway and bob not switching to their `_zoomed` values;
  - the viewmodel FOV change during zoom;
  - the Pilot ADS sequences (`ads_in_seq` etc.) being played where the game blends procedurally, or the other way round.
- *Research:* R3/R4 frame-by-frame ADS-in and ADS-out on 2–3 guns (R-201, Kraber, a pistol). R2 for each gun's `zoom_*`, `ads_*`, `viewmodel_offset_ads` and `_zoomed` keys.
- *Fix:* match the timing curve and the pose path.
- *Verify:* R5 ADS script, strips at 0, 0.05, 0.1 … 0.4 s.
- *Open question for the user:* which gun, and what exactly looks off (speed, path, snapping, position)? Default if no answer: start with the R-201.

**4. Titan dash ("boosts") feels weird** (B, medium)
- *Likely cause:* the dash speed curve, duration, the camera tilt (`player.rs` `DashTilt`), dash-pip regen and the sound/FX timing are partly reconstructed.
- *Research:* R2 for the titan `.set` dodge keys (`dodgeSpeed`, `dodgeDuration`, power drain/regen). R1 for the engine side. R3/R4 dash clips (distance covered, camera roll).
- *Fix and verify:* R5 dash script, comparing the distance-over-time curve.

**5. Re-entering the Titan gives it forward momentum** (A, high, quick)
- *Likely cause:* when the embark ends (`pilotctl.rs` ~490), BT's `state.vel` or a held input isn't cleared. BT keeps the velocity it had before, or picks up the animation's root motion.
- *Research:* reproduce with a script (walk, embark, stand still) and log BT's velocity before and after.
- *Fix:* zero BT's velocity when the embark ends (and when it starts), and swallow movement input until the keys are released again, if that's what the game does (check R3).
- *Verify:* script shows no position drift after embarking.

**6. Starting a wallrun doesn't always work** (B, high)
- *Likely cause:* the wallrun entry conditions in `tf-sim/src/pilot.rs` (~1066):
  - how far out the wall is probed, and in which direction;
  - the angle limits between view, velocity and wall;
  - the minimum speed or required input;
  - the "must be airborne for N ms" rule;
  - surfaces with bad normals in the render-mesh collision (see 8).
- *Research:* R2 for the `pilot_base.set` wallrun keys. R1 for `wallrun_*` ConVars. Write an automated sweep test in `tf-sim`: hundreds of jumps at the same wall at different angles and speeds, giving a success/fail map that shows exactly which cases fail.
- *Fix:* adjust the entry checks to the game's conditions.
- *Verify:* the sweep map is green where TF2 lets you wallrun.

**7. Sliding doesn't feel like TF2** (B, medium)
- *Likely cause:* slide speed boost, friction curve, minimum speed, slide-hop or slide-jump behaviour, camera height and tilt timing.
- *Research:* R2 for the `pilot_base.set` slide keys. R1 for `slide*` ConVars. R3 movement-tech videos with `cl_showpos` speed readouts (well documented: slide boost, slide-hop speeds).
- *Fix and verify:* R5 speed-over-time curve from slide start, compared with the clips.

**8. Falling through buildings in some places** (A, high)
- *Likely cause:* collision is built from the *render* triangles (`main.rs` ~588, `world::spawn_world`, plus static props). Titanfall 2 BSPs carry separate collision data (the `CM_*` lumps: brushes, primitives, grid, and `TRICOLL_*`) that the render mesh doesn't fully match: holes, one-sided surfaces, and missing props or models without render triangles. Fast falls can also tunnel through thin floors.
- *Research:* add a debug key that logs position and view to the log, so the user can mark the spots. Also run an automatic sweep: drop probes on a grid across the map and flag spots where a probe passes through a surface a downward ray says is there. Read the BSP v37 collision lump format (the community `bsp_tool` docs).
- *Fix:* build collision from the real `CM_*` and `TRICOLL` data. Make the player sweep tunnel-proof (substeps, or a swept capsule).
- *Verify:* the probe sweep shows no fall-throughs, and the logged spots hold.

**9. Grappling hook doesn't feel right** (B, medium)
- *Research:* R2 for `mp_ability_grapple` and its `.nut`. R1 for the `grapple_*` ConVars (pull force, max speed, detach rules, air control while attached). R3 clips.
- *Fix and verify:* R5 trajectory comparison for a grapple to a fixed point.

**10. Recoil on the Mozambique, and on sidearms in general, is weird** (B, medium)
- *Likely cause:* `vmmotion.rs` combines `viewkick_*` keys in a reconstructed way. Sidearms probably rely on keys we ignore or mis-weight (hipfire vs. ADS, weapon fraction vs. view fraction, perm vs. return, the spring choice in `springs.txt`).
- *Research:* R2, diffing the viewkick keys of the sidearms against the rifles to find what's different and unused. R3/R4 60 fps clips of a few shots (view kick vs. gun kick).
- *Fix and verify:* R5 strips of a 3-shot burst.

**23. BT slides too much when you hold forward and turn by looking around** (B, high, added after the first fix)
- *Likely cause:* in `tf-sim/src/titan.rs` (~254), BT's velocity moves toward the wanted velocity at one fixed rate for the whole vector. When you turn, the wanted velocity points somewhere new but has the same length, so the code takes the "slowing down" branch:
  - while sprinting it uses `sprint_decel` (400 u/s²), so a 90° turn at sprint speed takes over a second and BT drifts sideways;
  - while walking it uses `decel` (1500).
  Source-style movement handles this differently: friction removes speed in every direction, and acceleration only adds speed along the wish direction, so turning feels much tighter. Respawn's titan movement (the `.set` acceleration and deceleration keys) is engine code we can't read directly.
- *Research:* R2, checking which titan `.set` keys exist (acceleration, deceleration, and whether there's a turn or friction key) and what they're called. R1 for engine titan-movement ConVars. R3 clips of a titan sprinting and turning (strafing arcs, how fast the path bends). Also measure with the recorder: hold W, turn the mouse at a steady rate, and log the angle between where BT faces and where he's moving.
- *Fix:* split the velocity into the part along the wish direction and the sideways part. The sideways part is removed at the deceleration rate (or faster, if the reference shows that), and only the forward part uses acceleration. Then tune to the measurement.
- *Verify:* recorder run of sprint-and-turn, comparing the facing-vs-movement angle over time with the reference.

### Weapons, HUD, sights

**24. Tone's Sonar Pulse slot shows the Electric Smoke icon** (A, high, quick, added later)
- *Cause (found):* the kit table in `titankit.rs` (~294) asks the UI atlas for `rui/titan_loadout/tactical/titan_tactical_sonar_pulse_menu`, which doesn't exist (the game calls it `radar_pulse`). The lookup fails, so the slot keeps the Electric Smoke icon it was created with. The table also uses the loadout-menu art (`*_menu`), where the HUD should use each weapon script's `hud_icon` (`mp_titanability_sonar_pulse.txt`: `hud_icon "rui/titan_loadout/tactical/titan_tactical_radar_pulse"`).
- *Fix:* take every slot's icon from its weapon script's `hud_icon`, not a hand-typed path. Log any icon the atlas doesn't have, so a silent fallback can't happen again.
- *Verify:* screenshot of each kit's HUD (8 kits), compared against the scripts' `hud_icon` art.

**25. Tone's lock-on marks on enemy Titans are missing** (B, high, added later)
- *What the game does:* each 40mm hit, and a Sonar Pulse, gives 1/3 of a lock. A three-segment ring around the enemy fills one third at a time, and Tracker Rockets need the full ring. Sources: the Tone and Tracker Rockets pages on titanfall.wiki.gg. The game's own art is in `ui.rpak`:
  - `rui/hud/tone_tracker_hud/tone_tracker_1marks`: white, the lower-left third;
  - `tone_tracker_2marks`: white, the lower two thirds;
  - `tone_tracker_3marks`: the full ring, red, meaning locked;
  - `tone_tracker_core_marks`: red with an inner ring, likely during Salvo Core.
- *Ours:* nothing is drawn on the target; the count only shows as "LOCK n/3" text on the ordnance slot. The stacks themselves exist (`KitState.locks`).
- *Research:* the RUI that draws these is compiled, so placement and size need R3: gameplay footage of Tone hitting a Titan once, twice and three times. Check where the ring sits (the target's centre or its chest), how big it is (fixed on screen, or scaling with distance), the colour of 1–2 marks (white, or tinted), and whether it fades when a lock decays (ours drop after 10 s without a hit). Also check whether Sonar Pulse gives our targets a third, as the game does.
- *Fix:* draw the right image over each locked enemy's screen position, the same way the smart-ammo lock markers in `hud.rs` are placed.
- *Verify:* screenshots after 1, 2 and 3 hits on a Titan, next to reference frames.


**11. HUD countdowns don't behave like TF2** (C, medium)
- *Likely cause:* not pinned down yet. Candidates:
  - ability cooldown icons (fill style or direction, whether a number shows);
  - the dash pips;
  - the core meter;
  - the "Titan ready" countdown;
  - charge states (multiple charges);
  - the ready flash or sound.
- *Research:* R3/R4, recording the HUD right after using each ability and listing every difference from ours (`hud.rs`).
- *Open question for the user:* which countdown felt most wrong? Default if no answer: go through all of them against footage.

**26. Scorch's main gun can't light his gas canisters** (B, medium, added later)
- *What the game does:* Scorch's Incendiary Trap (`mp_titanability_slow_trap`) puts down a canister of gas, and hitting it with thermite (his T-203 shots, Firewall, Flame Core, the Thermal Shield) sets it alight.
- *Ours:* the canister only ignites on its own trigger, not from his shots.
- *Research:* the scripts for both the canister and the thermite launcher: how the trap's `OnDamaged` or the meteor's collision callback detects thermite damage and starts the burn (`mp_titanability_slow_trap.nut`, `mp_titanweapon_meteor.nut`).
- *Fix:* follow that path. Thermite projectiles or damage that touch the canister, or its gas, ignite it.

**27. Ion's Laser Tripwire beams are misaligned** (A, high, added later)
- *Likely cause:* the beams are drawn from a guessed height (`LASER_TRIP_CENTER` 85, noted as a guess in the f808eac/7b2cee0 work), not from the pylon model's attachments. The pylons themselves are still glow stand-ins, not the `titan_trip_wire` model.
- *Research:* the pylon model (`models/weapons/titan_trip_wire/titan_trip_wire.mdl`) and its beam attachment, plus how the script places the beam effect between pylons (`mp_titanability_laser_trip.nut`).
- *Fix:* draw the pylon model and run the beams between its attachment points.

**28. Northstar's Plasma Railgun zooms out when aiming down sights** (A, high, added later)
- *Likely cause:* the zoom applied while aiming, or the viewmodel lens during the zoom, is backwards for this gun. The railgun's `zoom_fov` may be read as a scale (or the other way round), or its `ads_fov_zoomfrac` keys mishandled.
- *Research:* compare the railgun's `zoom_fov` and `ads_*` keys with other Titan guns, and check what our camera FOV does during its ADS (log it).
- *Fix and verify:* FOV log and screenshots while zooming in.
- *Shelved (findings so far):* the world zoom is right (`zoom_fov` 50). The gun shrinks because the railgun's own `ads_in_seq` slides the gun and arms straight forward, from 39 to 78 units ahead of `jx_c_camera` (nothing moves sideways or up; `idle_ads_anim` is empty, and the zoom callbacks only handle Threat Optics). The other Titan guns hold their hip pose, and Legion's Predator comes closer (73 to 33). One attempt, zooming the whole viewmodel lens with the world, kept the gun's size but also zoomed the cockpit off the screen, and the gun grew and shrank during the transition (the FOV eases with a smoothstep while the animation follows the zoom fraction). It was reverted. Open question for later: does the real game keep the railgun's on-screen size (A), or does it slide forward and shrink (B)? If A, zoom only the gun's lens (a separate camera or layer from the cockpit) and time it to the animation.

**29. Titan projectile models point the wrong way** (A, high, added later)
- *Likely cause:* since 82eb7bf, bolts carry their script's `projectilemodel`, all turned with one fixed rotation copied from the Archer's missile, which assumed the model's nose runs along +Z. Most Source models point along +X, so the Titan rockets and shells fly sideways or tail-first.
- *Research:* each projectile model's bounds and attachments (`mdlinfo`/`attachdump`: the `exhaust` attachment the trail uses, and which axis is longest) for every Titan gun's `projectilemodel`. Also check which way the Archer's missile really points, since that's where the +Z came from.
- *Fix:* orient models by their real forward axis (very likely +X, and the same for all), and put the trail on the `exhaust` attachment like the game.
- *Verify:* screenshots of each projectile in flight from the side.

**12. Titan weapons lose their crosshair when zoomed** (A, high, quick)
- *Likely cause:* `crosshair.rs` (~142) fades every crosshair out by `adsFrac`. That is right for Pilot guns with sights, but Titan weapons in TF2 keep the crosshair while zoomed.
- *Research:* R3, to confirm which crosshairs stay and whether they shrink or tighten with zoom (they get `player_zoomFrac` as an argument).
- *Fix:* fade only where the game does (Pilot weapons with an ADS sight). Titan crosshairs stay.
- *Verify:* Titan ADS screenshot strip.

**13. Holding right-click with Legion shows an artifact on the weapon** (A, medium)
- *Likely cause:* one of the first-person effects added recently, drawn wrongly at zoom: the barrel spin `P_predator_barrel_blur_FP` (`PfxVmSpace`), the charge glow, or the Gun Shield's FP effect. A viewmodel RUI mesh is also possible.
- *Research:* capture Legion ADS frames and switch the effects off one at a time (R7) to find the culprit.
- *Fix:* correct that effect's space, scale or blend.
- *Verify:* ADS strip.

**14. On foot, the muzzle flash stays behind in the world for a moment when moving and shooting** (A, high)
- *Likely cause:* the Pilot's first-person muzzle flash is a hand-built world-space particle burst spawned at the muzzle's position (`pilotweapon.rs` ~1145). It doesn't follow the gun, and the game's `_FP` systems are skipped (`particles.rs` ~580) because they looked too big.
- *Research:* in the game, the view flash (`fx_muzzle_flash_view`) is attached to the viewmodel's muzzle attachment, so it moves with the gun. Check why the `_FP` systems came out oversized; the Titan's `PfxVmSpace` path now handles this correctly.
- *Fix:* play each weapon's `fx_muzzle_flash_view` on the viewmodel attachment through the viewmodel lens (`PfxVmSpace`), so it moves with the gun.
- *Verify:* strafe-and-fire strip.

**15. Thunderbolt scope looks different depending on looking up or down** (C, medium)
- *Likely cause:* something in the ADS/scope path depends on pitch:
  - viewmodel pitch offsets or sway clamps (`sway_*_pitch`);
  - the scope RUI's placement;
  - the viewmodel camera's lens (`vmcam.rs`) when pitched.
- *Research:* screenshots in ADS at pitch −60, 0 and +60 to show which part moves. R2 for the Thunderbolt's ADS keys.
- *Fix and verify:* the same three screenshots look the same.

**16. Archer has no proper sight** (C, medium)
- *Likely cause:* sights and reticles drawn by RUI aren't implemented (`ruiscreen.rs` only draws ammo counters).
- *Research:* the Archer's `UiData`/scope RUI. Its reticle art in the RUI atlas (`rui/...`), as `crosshair.rs` did for crosshairs. R3 for the look, including the lock-on state.
- *Fix:* draw the Archer's scope reticle on its sight mesh, then do the same for the other weapons with RUI sights.

### Visuals and rendering

**17. Smoke and other see-through effects (shields etc.) still look wrong** (C, high)
- *Likely cause:* the particle and shield shaders don't match the materials:
  - `.vmt` blend flags (`$additive`, `$translucent`, `$overbrightfactor`, `$softparticles`);
  - HDR scale;
  - particle lighting;
  - draw order between transparent things;
  - soft-particle depth fade.
- *Research:* R2, building a table of every material the common effects use and its flags. R6 RenderDoc on our frame. On the real game's frame if R4 is possible, the blend states are visible directly.
- *Fix:* one shared, data-driven particle/translucent shader that follows the material flags.
- *Verify:* side-by-side shots of smoke, the Vortex dome, the Particle Wall and the Gun Shield.

**18. In some places the floor is drawn over explosions and shields** (C, high)
- *Likely cause:* a depth or draw-order problem. Candidates:
  - some world surfaces render in the transparent pass (blend or decal materials) after the particles;
  - particles drawn by a camera or layer whose depth doesn't include those world meshes;
  - a depth bias on decal layers.
- *Research:* log the spots (same debug key as item 8) and use R6 to see which mesh draws over the effect and in which pass.
- *Fix:* put those world materials in the correct pass, or give particles the right depth test against them.

**19. Tone's Particle Wall curves the wrong way when placed** (A, high, quick)
- *Likely cause:* the wall's model or effect is oriented 180° wrong (its forward axis flipped), so it bulges toward the player and away from the enemy, or the reverse.
- *Research:* the Particle Wall model's forward axis, and the script's angles (`mp_titanability_particle_wall.nut`). R3 to see which way it bows in the real game.
- *Fix:* flip the orientation.
- *Verify:* a placement screenshot from the front and the side.

**20. Holo Pilot moves weirdly** (C, medium)
- *Likely cause:* the decoy's animation choice, blending or speed doesn't match its movement (foot sliding), or its path behaves differently (`pilotability.rs`).
- *Research:* `mp_ability_holopilot.nut` (how the decoy is set up, its speed, it copying your movement). R3 clips of a decoy running.
- *Fix:* drive the decoy's run animation from its actual speed, and follow the script's behaviour.

**21. Lighting is off** (deferred, as you asked)
- Not touched now. It will need its own research pass later (lightmaps, sky and sun, exposure, tonemapping).

### Not a bug

**22. No audio in the first play session.** I launched the game through the muted test wrapper; the second launch had sound. The log also lists sound events that have no sound found (e.g. `Pilot_Rodeo_Titan_Attach`, `weapon_predator_rangeswitch_*`). That is a separate, minor cleanup.

## Working order: easiest to hardest

We work through these one at a time, and the next one starts only after the user has tested and approved the current one.

1. ~~Re-entering the Titan gives forward momentum (item 5)~~ (fixed in 76b3aa5: a leftover dash press from jumping into BT)
2. ~~Tone's Particle Wall curves the wrong way (19)~~ (fixed in 01b4830)
3. ~~Tone's Sonar Pulse shows the Electric Smoke icon (24)~~ (fixed: icons from the scripts' hud_icon, kept at their own aspect)
4. ~~Titan weapons lose their crosshair when zoomed (12)~~ (fixed: only Pilot guns fade theirs)
5. ~~Titan projectile models point the wrong way (29)~~ (fixed: models placed by their bone pose, nose along +X)
6. Northstar's railgun zooms out when aiming (28) (shelved by the user; findings under item 28)
7. ~~Ion's tripwire beams are misaligned (27)~~ (fixed: beams left to middle to right at the pylons' 45-unit rise, tower model, path particles reach the end)
8. ~~Tone's lock-on marks on enemy Titans (25)~~ (done: the tracker ring on Titans, 60 s marks, Sonar Pulse marks and flies as an echo; the echo's flight path is pinned: the user finds it odd, cause not found yet)
9. ~~Legion right-click artifact on the weapon (13)~~ (fixed: the barrel blur sleeve lay across fx_barrel's X instead of down its Z; it and every effect model drew a frame late. Follow-up: the sleeve is hidden behind the gun, as effect models draw in the world pass, and its vertex alpha and fade-in are missing)
10. ~~On-foot muzzle flash stays behind in the world (14)~~ (fixed: FP muzzle flash and shell eject play on the viewmodel joints)
11. ~~Thunderbolt scope depends on looking up or down (15)~~ (fixed: the walk layer played the Thunderbolt's 67-frame standing clip, which turns the gun 36 degrees for half its 2.2 s loop, at full weight while standing; the layer now fades in with speed)
12. ~~Scorch's gun can't light his gas canisters (26)~~ (fixed: the T-203 shell's blast and its new physics thermite (Proto_MeteorCreatesThermite) ignite the gas)
12b. ~~Scorch's Thermal Shield catches rounds and its fire doesn't hurt shielded enemies (added later)~~ (fixed: it destroys rounds (P_wpn_HeatShield_impact), burns every 0.1 s within 175 of the hand and 70 degrees of the view; its burn, thermite and trap fire are radius damage that a Vortex/Thermal/Gun Shield doesn't stop; trap segments tick every 0.2 s within 240 of the canister. `TF_STATIC_TITAN=1` spawns one standing, unkillable test Titan)
13. ~~Titan HUD moves far too much (1)~~ (fixed from client.dll: cockpit sway eases at cockpitSwayGain per second toward a target clamped to +-1 unit / +-0.5 degrees, fed by view-space velocity x frametime and the 5-frame turn rate x tick interval; footstep jolts are spring velocity kicks (-5 u/s, x2 sprinting); CockpitJolt moves severity units toward the hit plus 100x as velocity; the dash tilt follows the heading under a 0.7 s smoothstep at 5/2.5 per second)
13b. ~~Vortex doesn't throw caught rounds back on release (added later)~~ (fixed: only weapons with vortex_refire_behavior "absorb" (Ion's Splitter Rifle) are absorbed; everything else is caught (up to 32) and fired back on release. Still approximate: the return fire is hitscan bullets at 140 each, where the game sends bullets back as one shotgun blast and rockets as real projectiles)
14. ~~BT slides too much when turning while holding forward (23)~~ (fixed: Respawn's WalkMove from client.dll: friction keeps the velocity along the wish direction up to the wish speed and brakes only the rest at the (sprint) deceleration; acceleration then adds along the wish direction without raising the speed above the old speed or the wish speed)
15. ~~Titan dash feels weird (4)~~ (fixed from client.dll's jump handler: dodgeSpeed along the input (view forward with none), the old velocity keeps what exceeds it; BT is airborne for dodgeDuration with no speed clamp at the end, ending early below dodgeStopSpeed; dodgeInterval counts from the last dash's start; the hop is added in the air too; AirMove with airSpeed 60 / airAcceleration 500 and the 0.5 s keyboard grace)
15b. ~~Caught Vortex rounds stay where they were caught instead of moving with BT (added later)~~ (fixed: Vortex_SpawnImpactAbsorbFX: each caught round's vortex_absorb_effect (owner) / _third_person plays parented to BT with CP1 at the bullet collection point (300, -90, -70 from the eye), until the shield goes down)
15c. ~~Raising the Vortex during a reload shows three arms (added later)~~ (fixed: an offhand becoming the active weapon deactivates the gun (client.dll 0x1805a3fd0: weapon state idle, m_bInReload cleared), so the reload is cancelled; the last AE_WPN_RELOAD_MILESTONE passed is kept and the next reload of the same kind plays reload_(empty_)lateN_seq over reload(empty)_time_lateN)
15d. ~~BT's shield arm pose is missing in third person (added later)~~ (fixed: bt_vortex_arm_pose_add is a full-pose gesture limited by its weightlist, blended over the aim layer; the 3P shield rides the raised arm at w_vortexblocker's vortex_center. BT's NPC set is the mirror of the cockpit (gun left, Vortex right), so by choice he plays mirrored copies: gun in the right hand, Vortex in the left. In the cockpit the gun's arms take the left arm from the blocker viewmodel, so two arms show, not three)
16. Everything moves too much and is too shaky (2)
17. HUD countdowns don't behave like TF2 (11)
18. ~~Mozambique and sidearm recoil (10)~~ (fixed: the gun's share of the weapon punch turns the viewmodel about the eye, as client.dll does, not about the far ADS sway pivot, which swung the gun down off the sight. Kick reconstructed from client.dll: one weapon punch spring split by weaponFraction, random kick at half the `random` range, roll not scaled by the kick ramp, the engine's defaults for missing keys, viewmodel_shake driven by the punch roll, and attack_seq fading in over its 0.05 s)
19. ADS animation is weird (3) (shelved by the user for now)
20. ~~Holo Pilot moves weirdly (20)~~ (fixed: it played Run_forward_mp's first blend cell, a run turned 90 degrees right; single-clip sequences now take a grid's neutral cell, which also straightens the enemy Titans' walk. The decoy follows server.dll's CPlayerDecoy: state copied from the player, root-motion movement, walls, ledges, landing, slide and wallrun, carrying the player's gun. The third-person wallrun clips were swapped: they're named for the lean)
21. Wallrun start is unreliable (6)
22. Archer has no proper sight (16)
23. Sliding doesn't feel like TF2 (7)
24. Grappling hook doesn't feel right (9)
25. Floor drawn over explosions and shields (18)
26. Smoke and see-through effects look wrong (17)
27. Falling through buildings (8)
28. Lighting (21, parked until the user asks)

## Order of work (original grouping)

1. **Quick, concrete bugs:**
   - 5: embark momentum;
   - 19: Particle Wall curve;
   - 12: Titan zoom crosshair;
   - 14: on-foot muzzle flash;
   - 13: Legion artifact.
   Each is small, testable and visible.
2. **Research setup, in one go:**
   - R1: ConVar dump from the binaries;
   - R2: list of unused keys;
   - R5: telemetry and side-by-side harness;
   - R7: kill switches.
   Everything after this depends on it.
3. **Feel:**
   - 2 and 1: shake and HUD sway, which fix the biggest "jank" impression;
   - then 3 (ADS), 10 (recoil), 6 (wallrun entry, with the sweep test), 7 (slide), 9 (grapple) and 4 (dash).
4. **Collision:**
   - 8: real BSP collision.
   It's big, but falling through floors breaks play, so it might move up to step 2.
5. **Rendering:**
   - 18: depth and draw order;
   - 17: translucent effects;
   - then 15 (Thunderbolt scope), 16 (Archer sight), 20 (Holo Pilot) and 11 (countdowns).
6. **Lighting** (21), when you say so.

## Open questions (with defaults if unanswered)

- **Can you record short clips in the real TF2, or may I set up Northstar for an offline private match with a console (R4)?** Default: no. I'll use public footage and the binaries.
- **ADS:** which gun and what looks wrong? Default: start with the R-201.
- **Countdowns:** which one felt most wrong? Default: compare all of them against footage.
- **Fall-through spots:** if you remember where (which building or area), tell me. Default: the automatic probe sweep finds them.
