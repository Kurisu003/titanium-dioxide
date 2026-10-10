# Titanium Dioxide

A fan-made, from-scratch game engine written in Rust (Bevy + wgpu) that is **compatible with
Titanfall 2**: it loads the maps, models, animations, sounds, effects and gameplay scripts'
numbers straight from **your own installed copy** of Titanfall 2 and plays them in a new engine.
The goal is to recreate the game's mechanics one to one: movement, weapons, Titans, animations
and visuals.

> **Early work in progress.** This is a first public version. Expect rough edges, missing
> features and things that don't feel quite like the real game yet.

## Legal notice

- **You must own Titanfall 2.** Titanium Dioxide does not include, and will not run without, the
  game's files. It reads them at runtime from a legally purchased and installed copy of
  Titanfall 2 (for example the Steam version). Buy the game to use this project.
- **No game assets are included or distributed.** This repository contains only original source
  code (plus clearly marked third-party code listed under [License](#license)). It contains no
  models, textures, maps, sounds, music, videos, scripts, keys or other files from Titanfall 2,
  and it does not modify, patch or redistribute the game. Please don't open issues or pull
  requests that add game files; they will be removed.
- **Not affiliated.** This is an independent, non-commercial fan project. It is **not affiliated
  with, endorsed by, sponsored by or approved by Respawn Entertainment, Electronic Arts (EA) or
  Valve**. "Titanfall", "Titanfall 2" and related names and logos are trademarks of Respawn
  Entertainment and/or Electronic Arts Inc. All game content belongs to its respective owners.
  Names of the game are used only to describe what this project is compatible with.
- **Not for online play.** Titanium Dioxide is an offline, single-player engine. It does not
  connect to, emulate or interfere with Titanfall 2's official servers, accounts, anti-cheat or
  online services.
- **No warranty.** The software is provided "as is", without warranty of any kind. Use it at
  your own risk.
- **Rights holders:** if you represent Respawn Entertainment or Electronic Arts and have any
  concern about this project, please open an issue on this repository (or contact the author
  through GitHub) and it will be addressed promptly.

## Requirements

- A legally owned, installed copy of **Titanfall 2** (PC).
- [Rust](https://rustup.rs) (stable) and a GPU with Vulkan, Metal or DirectX 12 support.
- Developed and tested on Linux. Other platforms may work but are untested.

The game install is found automatically in your Steam libraries. If it isn't, pass
`--game /path/to/Titanfall2` or set the `TITANFALL2_DIR` environment variable.

## Running

```
cargo run --release -p tf-viewer -- [--game /path/to/Titanfall2] [--difficulty easy|normal|hard|master] [--map mp_forwardbase_kodai]
```

Use `--release`. The world collision BVH (7.5 million triangles) is built in parallel at load, which takes about 1.5 s,
and combat then runs at roughly 80-95 fps. Debug builds are also optimized and run at about 60-80 fps.

Press ENTER on the title screen. Waves of enemy Titans come at you (one more every other wave, up to six), mixing Ion,
Scorch, Northstar, Ronin, Tone and Legion so that every class shows up before any repeats;
survive as long as you can. If BT is destroyed you eject, and T calls in a new Titanfall once the
rebuild timer runs out. If the pilot dies, the run is over.

Controls: click to grab the mouse (Esc releases), WASD moves, Shift sprints (a tap latches it until you stop moving forward, crouch, aim or fire, as in the game), Space dashes (or jumps on foot), Ctrl
crouches/slides, LMB fires, RMB zooms, R reloads, hold G to paint missile locks, V fires Burst Core,
hold Q for the Vortex Shield, E uses the utility (Electric Smoke on BT's default kit), F punches, X disembarks/embarks, T calls a Titanfall, C toggles cockpit/third
person (the game starts in the cockpit, as Titanfall 2 does), and F1 toggles a free camera. `-` and `=` lower and raise the volume. It starts quiet (15%);
`--volume 0.5` sets it at launch. The master volume is applied while mixing, so it changes sounds
already playing (loops, music) too.

## What works

- **Titan combat (BT-7274):** cockpit view with the game's cockpit model, HUD power-on, sway and
  damage jolts; walking, sprinting and dashing with the game's numbers; the XO-16 and all eight
  Titan kits (Expedition, Scorch, Ion, Northstar, Ronin, Tone, Legion, Brute) with their
  weapons, defensives, utilities, ordnance and cores; melee; the Vortex Shield; Titanfall
  call-ins, ejecting and the rebuild timer.
- **Pilot movement:** walking, sprinting, crouching, sliding, jumping, double jumps, wall-running
  and wall jumps, mantling, ziplines and the grapple, using the values from the game's player
  settings files.
- **Pilot weapons:** all 22 primary weapons, plus sidearms and anti-Titan weapons, with their
  viewmodels and animations, aiming down sights, recoil, spread, reloads, sounds, muzzle flashes,
  tracers and impact effects taken from the weapon scripts.
- **Pilot abilities:** tacticals (stim, grapple, pulse blade, holo pilot, A-Wall, cloak and phase shift) and
  ordnance (frag, arc, firestar, gravity star, satchel, electric smoke).
- **Embarking and disembarking** with the game's first-person animations, **rodeo** and
  batteries, and **executions**.
- **Maps:** Titanfall 2 multiplayer maps (Forwardbase Kodai by default) and several campaign maps,
  with props, lighting, reflections, skyboxes and fog.
- **Effects:** the game's own particle systems (a large share of their features), decals and
  impact tables.
- **Sound:** the game's sound banks: weapons, movement, Titans, dialogue with subtitles and music.
- **Front end:** title screen, menus, loading screen and a wave-survival mode against enemy Titans
  and infantry (simple AI, mainly there to have something to shoot at).

## What doesn't work yet

- **Weapon sights and scopes:** the reticles are compiled UI programs that can't be run yet, so
  optics show no reticle.
- **Some visuals:** refraction is approximated, and only for particle effects (heat haze,
  shockwave rings); shield domes and walls use the game's effect shader but leave out its height fog; some particle features are missing; lighting, fog and
  materials are approximations, not the game's shaders.
- **Movement details** that the game's engine code (not its data files) controls: the formulas
  behind the movement keys (we follow the game's own descriptions of them) and
  the fine "feel" are approximated.
- **AI:** enemy Titans and grunts are basic. There is no campaign, story or mission scripting.
- **No multiplayer** of any kind.
- **Platforms:** only tested on Linux.
- Many smaller gaps are listed under [Known gaps](#known-gaps) below.

## License

Titanium Dioxide's own source code is licensed under the
**[PolyForm Noncommercial License 1.0.0](LICENSE)**: you may use, copy, modify and share it for
any non-commercial purpose, but **not sell it or use it commercially**, and you **must keep the
copyright notice / credit to the author** (Daniel, [Kurisu003](https://github.com/Kurisu003))
in anything you share.

Third-party code keeps its own license:

- `crates/tf-assets/third_party/lzham_alpha`: LZHAM decompressor, MIT license (see its
  `license.txt`).
- `crates/tf-assets/src/rpak_decompress.rs` and `rpak_lut.rs`: a port of the RPAK decompression
  routine as published by [r-ex/rsx](https://github.com/r-ex/rsx), which is licensed under the
  GNU AGPLv3; these two files are under the AGPLv3.

## Credits

File-format knowledge comes from the Titanfall modding community's open-source work, especially
[r-ex/rsx](https://github.com/r-ex/rsx), [r-ex/RePak](https://github.com/r-ex/RePak),
[snake-biscuits/bsp_tool](https://github.com/snake-biscuits/bsp_tool) and
[barnabwhy/sourcepak-rs](https://github.com/barnabwhy/sourcepak-rs). Thanks to Respawn
Entertainment for making Titanfall 2.


## Technical notes

Detailed notes on how each system works and where its numbers come from. Anything marked as a guess or approximation is not taken from the game's data.

### Effects

The game's own particle systems play (`pfx.rs`; `tf_assets::pcf` parses the 396 `particles/*.pcf`
files, Valve DMX binary 5, into 10 437 system definitions): emitters, initializers, operators,
forces, constraints and renderers are compiled from each system's operator list with Source's
defaults, children start with their delays, and every live particle of a material goes into one
camera-facing mesh per frame. Generic effects map to the game's systems and impact tables
(`scripts/impacts/<table>.txt`: `wpn_muzzleflash_xo`, `P_wpn_tracer`, `titan_bullet`,
`exp_rocket_shoulder`, `xo_exp_death`, `titan_landing`, ...). Hits play each weapon's own
`impact_effect_table` for what the round struck (`Effect::Hit`): metal_titan (`E`) on a Titan,
flesh (`F`) on a Pilot or grunt, `X`/`shieldhit` on a Vortex, Particle Wall, A-Wall or Gun
Shield, concrete otherwise; so the XO-16 sparks `impact_metal_titan` off an enemy Titan, Tone's
40mm bursts `P_impact_exp_med_metal`, Ion's rounds `P_impact_accel_titan`, and weapons without a
table (grunt rifles) use `default`. A hit on the player plays the table's `FX_victim` entry
instead when it has one (`P_impact_accel_victim`, `P_impact_titan_sniper_victim`,
`P_impact_leadwall_victim`, `P_impact_metal_victim_elec`: the game's first-person versions).
Sprite materials honour `$depthblend`/`$depthblendscale` (soft particles: `soft.wgsl` fades a
sprite over that many units of depth between it and the scene behind it, read from the depth
prepass SSAO already needs) and `$ignorez` (drawn over everything); `TF_NO_DEPTHBLEND=1` and
`TF_DEPTHBLEND_SCALE=k` are for checking it. `Set Control Point To Player` puts a control point
(1 by default, as in Source) at the camera. Operators and initializers honour Respawn's
`operator end cap state` (1: only once the effect has been stopped, 0: never then), which is
how a held shield's fan lives until it is dropped and the gun shield only then picks up its
5 s lifetime and fade (stopping an effect re-runs its end-cap initializers on the living
particles; a guess at the engine's behaviour). `Graph Scalar`'s `output op` 1 scales the
initial value and 3 the current one (both a multiply here, since fields reset to their base
each frame). The second alpha field (16, which 1244 of the game's operators write) multiplies
alpha, starting from 1: Respawn's renderers post-multiply it or erode the texture by it, and
the erosion is not modelled. `Remap Scalar` (creation time read as the particle's age),
`Remap Initial Scalar`, `Remap Distance to Control Point to Scalar`, `Remap Distance Between
Two Control Points to Scalar`, `Oscillate Scalar` and `Ramp Scalar Linear Simple` follow
Source's operators; the last two integrate per frame in Source and use the closed form here.
`Rotation Orient Relative to CP` rolls particles away from the point in its own XY plane
(Source uses world XY). `Rotation Spin Roll` eases to `spin_rate_min` over `spin_stop_time` seconds, `Oscillate Vector`
swings the colour (its default field) and `Restart Effect after Duration` re-runs a system's
emitters every min..max seconds. `Position From Parent Particles` births a child system's particles on
random living particles of its parent, inheriting their velocity scaled.
Particles also carry a yaw, a normal and (for models) a spin: `Rotation Yaw Random` and
`Rotation Spin Yaw` set and turn the yaw, `Normal Align to CP` takes a control point's forward
axis as the normal, `Normal Lock to Control Point` turns it with the point and `Remap Velocity
to Vector` (into the normal) makes it follow the motion. `Render models` now orients each
model by its particle: Z on the normal when "orient model z to normal" is set (else control
point 0's forward, as before), then the particle's yaw about Z and roll about X (Source's
AngleMatrix with pitch 0; which axis the roll uses is a guess), the model-space angles and
Respawn's `Rotation about 3D Axis` spin about a model-space axis (its never-written defaults,
the Z axis and 360 degrees/s, are guesses; "modulate by roll speed" is not applied). Debris
chunks and caught Vortex rounds tumble instead of all sharing the control point's frame.
Remaps into roll or yaw take degrees, as Source's do. `Inherit Initial Value From Parent
Particle` and `Inherit Attribute From Parent Particle` copy a field (position, lifetime,
radius, alpha, colour, roll, yaw, trail) from the parent's particle with the same index;
`Remap Particle Count to Scalar` maps the particle's number ("from end of list" is ignored);
`Set CP Position` places a control point in another's frame; `Set (child) control points from
particle positions` moves control points onto a node's first particles (with "set orientation"
facing their normal or motion), which is how a shell eject's resting child follows the flying
casing; `Remap Percentage Between Two Control Points to Scalar`, `Ramp Scalar Linear Random`,
`Oscillate Scalar Simple` (per-particle closed form; Source runs it on the system clock),
`Lerp EndCap Scalar` (after the effect is stopped), `Stop Effect after Duration` (which stops
its children's emission too, as Source's StopEmission does, with "Control Point to Scale
Duration" scaling it by a control point's X when one is set), `Movement
Rotate Particle Around Axis` and `Movement Maintain Position Along Path` (no bulge or cohesion)
are in too. `Remap Control Point to Vector` (into the colour) and `Remap Control Point to
Scalar` read only control points that hold a value (passed by the caller or set by an
operator): the others copy control point 0's position, where the game's scripts would have set
a colour or a scale, so those systems keep their own colour. Sprites ignore the normal and yaw
(orientation 3 still faces control point 0's up). `TF_PFX_NO_ALPHA2=1`
ignores the second alpha, for comparing. System colours are squared, (byte / 255)^2, as
client.dll's `Color Random` setup does (0x180706700); taken as linear they had washed every
effect out, drawing a frag's dark dirt smoke as a pale tan haze (`TF_PFX_GAMMA_COLORS=1`
shows that look). `Color Random`'s lighting tint is in (client.dll 0x1806fa030; 2365 of the
game's 4768 `Color Random`s use it): with `tint_perc` set, the light at the tint control point
times `light amplification amount` is blended into the colour by `tint blend mode` (0 the
light, 1 overlay, 2 darken, 3 lighten, 4 multiply; higher modes write nothing) and lerped in by
`tint_perc`, re-read once the point moves `tint update movement threshold` units. The light is
engine.dll's no-normal `GetLightForPoint` (0x180175ea3) on the map's own data: the nearest
light probe's L0 (int16 / 8192), plus a quarter of the sun's `WORLD_LIGHTS` intensity times the
probe's sun visibility, plus the sky ambient's times |v| (|v| + 1) / 4 for the probe's sky
vector v (times `light sky scale`). Approximations: the nearest probe instead of the probe
tree, the probe's local lights and two engine scalars on the sun term left out (taken as 1).
So smoke and dust are lit by where they are: the Titan dash's grey dust (61-165 grey times
twice the light, at 50%) is pale in daylight where its plain colour drew dark blobs.
`Color Lit Per Particle` keeps plain colours. `TF_PFX_NO_TINT=1` turns the tint off. Texture names with a source-art
extension (`dirt_burst_full.tga`) load their .vtf; those dirt bursts were missing.
Particle textures are precached the way the engine precaches each weapon's systems: once
the library has loaded, its thread walks every system the weapon scripts name (`fx_*`,
tracers, trails) and every system in the impact tables they use, with children (156
materials, about 0.25 s on all cores), so the first explosion no longer stalls the frame (the
first frag took 350 ms: Respawn's VPKs are LZHAM-compressed and a 512 mip is the last thing in
its file). Materials not in that set decode on all cores when first used.
`TF_PFX_NO_PRECACHE=1` turns it off. The Pilot loadout's viewmodels are parsed on all cores
and built (with BT's kit) during a few extra frames under the loading screen; built on the
first playing frame they froze the game for a second as it began. `RUST_LOG=tf_viewer::ui=debug`
logs frames slower than 60 ms after loading. `TF_PFX_TEST=<system>` with the script action `pfxtest` plays a system on the ground 300 units
ahead of the camera (`TF_PFX_TEST_DIST=units`; the ground is found by a trace down, so it works
from the cockpit and a `--cam` free camera), for checking one in isolation; `TF_PFX_SKIP=a,b`
leaves out child systems whose names contain a part. Effects carry 16 control points (the game addresses up to CP 10). `Render models`
renderers draw a model per particle (`fx_model_materials`: unlit effect materials draw with
the game's own unlit uber shader, see Shield looks; `TF_OLD_SHIELDS=1` brings back the earlier
additive / translucent stand-ins; `TF_NO_FX_MODELS=1` hides them and `TF_FX_MODEL_SCALE=k` rescales them for checking). Sprites also fade out within 60 units of the eye (rounds and smoke
arriving at the camera would white the screen out). `$overbrightfactor` (up to 30 on the
`_15ob`/`_30ob` glows) is applied as its square root: at full strength every glow is a flat
white disc under this tonemapper, so the camera's bloom carries the rest (an approximation
of the game's HDR pipeline, not its numbers). Model renderers draw (shield walls, casings).
`Emit Decal` lays a bullet hole on level geometry: one of the system's materials (its
`_subrect` atlas name maps to the material without the suffix), 4 units wide (the size is a
guess), the newest 256 kept; `TF_NO_DECALS=1` turns them off. Translucent world surfaces (the
BSP's translucent meshes: ground dirt overlays such as `dirt_pathway_to_transparent_bm`, which
covers much of Kodai's floor, grime decals, glass) and these decals draw before every other
translucent thing (`convert::TransparentOrder`, applied to Bevy's transparent phase before it
is sorted). Bevy adds a material's `depth_bias` (256 on overlays, against z-fighting) to the
sort distance too, so they had sorted 256 m nearer and drew over explosions, smoke and shields
wherever they lay. The cost: an effect behind a translucent window draws over the glass, and
refracting sprites (drawn before the transparent phase) are still covered by those overlays. Refracting materials (the
`Refract` shader: heat haze, explosion shockwave rings, distortion rings) draw in Bevy's
transmissive phase (`RefractMaterial`, refract.wgsl): a copy of the scene drawn so far (world
and sky) is looked up offset by the material's normal map as in Source's Refract shader (the
screen-space offset is the normal's XY times `$refractamount`, the normal map's alpha and the
particle alpha; `$vertexcolormodulate` tints, `$bluramount` is a 4-tap blur, `$ignorez` skips
the depth test). Guessed: Respawn's `$opacitymap` is read as a mask that scales the offset and
blends the sprite; other effects' sprites aren't refracted (they draw after the copy, over the
refraction); the first-person overlay leaves refraction out. `TF_NO_REFRACT=1` turns it off and
`TF_REFRACT_SCALE=k` multiplies the amounts (for checking). Screen effects (systems the
scripts start at the world origin or on the cockpit whose `screen space effect` flag is set:
Phase Shift's shimmer, Stim's streaks, the health hexes) run in screen units around their
control point and draw on a plane 8 units in front of the eye through the world camera, so
they refract: x ±100 spans the width (the systems' boxes are ±100 by ±63, a 16:10 screen) and
y runs up (a guess; Source's own screen-space pass isn't known), with the viewmodel drawn over
them. World systems that carry the flag too (lens flares, light glows) are drawn where they
are. `TF_NO_SCREEN_FX=1` leaves screen effects out. Light-source particles are off by default
(`TF_PFX_LIGHTS=1`: with the map's irradiance volume loaded, any visible point light makes Bevy
0.18 draw BT in the wrong pose, unlit). `TF_OLD_FX=1` keeps the hand-built look below, which is
also the fallback while the library loads, for smoke puffs and for first-person muzzle flashes
(the game's `_FP` systems are sized for its viewmodel FOV and fill the screen here). `pcfls` lists a file's systems.

The hand-built effects are particles drawn with the game's SpriteCard textures
(`materials/particle/...` VTFs in the VPKs, decoded by `tf_assets::vtf`):

- Flipbook sequences come from each texture's sprite sheet (for example the 16-frame
  `exp_fire_ball` and `exp_burst`).
- Alpha-only textures get the colour ramp their material names (`$RAMPTEXTURE` with
  `$texColorFromAlpha`, for example `flash_cloud_fire` for muzzle flashes and `fire_ramp` for
  fireballs).
- Additive or blended, per the material.
- All live particles of a texture share one camera-facing mesh rebuilt each frame (one draw
  call per texture, capped at 6000 particles).
- Energy weapons (Splitter Rifle, Plasma Railgun) get the blue ramps. Thermite and 40mm shells
  leave smoke trails and explode.
- `TF_FX_TEST=1` cycles the big effects in front of the camera for checking them.

### Weapons, maps and movement

- **Pilot arsenal:** every pilot weapon is built from its weapon script. That covers auto,
  semi-auto and burst fire, clip and reload times, spread and kick, ADS zoom, hitscan or
  projectile rounds (with gravity and explosions), and the shotgun bolt patterns from each
  weapon's `.nut`.
  - Each gun has its own first-person viewmodel with the pilot's arms, and its own fire sounds.
  - You carry a primary, sidearm and anti-Titan weapon. Switch with 1/2/3 or the mouse wheel;
    holster and deploy use the weapons' own timings.
  - Pick the loadout in the menu, or use `--loadout mp_weapon_hemlok,mp_weapon_wingman,mp_weapon_defender`.
- **BT's loadouts:** pick one in the Loadout menu (or `--titan 0..7`), or in the Titan keys 1-8 switch between Expedition (XO-16), Scorch (Thermite
  Launcher), Ion (Splitter Rifle), Northstar (Plasma Railgun), Ronin (Leadwall), Tone (40mm),
  Legion (Predator Cannon) and Brute (Quad Rocket).
  - Each uses its script's numbers and its `.nut` projectile speed and bolt pattern.
  - The Predator Cannon is `attack_button_presses_ads`: the trigger zooms in, which is its
    wind-up (`zoom_time_in` 1.5 s, the length of its `ads_in_seq`, which swings the gun down
    from its raised carry pose; the script spins the barrels, `P_predator_barrel_blur_FP` on
    `fx_barrel`, from the start of the zoom), and it fires once fully zoomed.
  - Each has its own viewmodel and in-hand model.
  - The Plasma Railgun charges while you aim down sights (`charge_time` 2.25 s over 5
    `charge_levels`, draining in 0.5 s), ticking at each level. A shot does 250 plus 300 per
    charge level against Titans (`GetTitanSniperChargeLevel`, `damage_additional_bullets_titanarmor`):
    550 from the hip, 2050 at full charge. The level also picks the fire sound and scales the kick
    as `FireSniper` does.
  - The Splitter Rifle fires three bolts side by side in ADS (`boltOffsets` 0, ±0.022). The
    shared-energy cost isn't modelled.
- **Charge weapons:** the pilot Charge Rifle charges for 1.3 s while the trigger is held and fires
  only when full (as `OnWeaponPrimaryAttack_weapon_defender` requires); letting go drains it over
  1 s. It has no magazine: its 10 rounds are the stockpile (`ammo_default_total`). The Cold War
  charges for 0.5 s and then fires its burst (`charge_require_input` 0). A meter under the
  crosshair shows the charge, one pip per level.
- **Hitboxes:** the MDL v53 hitbox sets (box per bone with its hit group and crit flag) are
  parsed, and the player's hitscan and projectiles trace the enemies' posed boxes. Head hits
  on infantry use `damage_headshot_scale`; Titan crit spots use `critical_hit_damage_scale`
  (`titanarmor_critical_hit_required` where set). A grunt headshot with the R-201 is 25 x 3.
- **Weapon mods:** `Mods` blocks of the weapon scripts apply (`*x`, `/x`, `++x`, `--x`, replace).
  `TF_MODS="mp_weapon_r97:pas_fast_reload,extended_ammo;*:pas_fast_swap"` picks mods per weapon
  (`*` for all); unknown names are logged with the weapon's mod list.
- **Smart ammo** (`smartammo.rs`, from `sh_smart_ammo.gnut` and the `smart_ammo_*` keys):
  - The Smart Pistol searches a 35 degree cone to 1250 units (line of sight, Titans block it),
    starts a lock 0.1 s after seeing a target and takes 0.1-0.2 s per lock on NPCs (the
    `_npc` times; range stretches the time), as many locks per grunt as its health over the
    near damage at the 2x head-shot factor, 12 across the burst. A trigger pull fires one round
    per lock straight at the head box (`HEADSHOT`), then unlocks; without locks it fires
    normally. `fast_swap_to` brings it up at once.
  - The Archer (`attack_button_presses_ads`) aims when the trigger is pressed and fires only at
    full zoom, locking Titans only while aiming (9 degree cone, 6500 units, 1.05-1.55 s). Its
    rocket is the script's projectile model at 1750 u/s turning 70 degrees/s onto the target
    (the SP numbers in `OnWeaponActivate_weapon_rocket_launcher`), and fires unlocked in SP.
  - HUD (the game's is engine RUI, so recreated with its art): the Smart Pistol's search circle
    around the crosshair, `smart_ammo_corner` brackets closing on each target as the lock forms
    and the lock count beside it. Lock sounds are the script's events. The Archer's locks show
    only on its sight screen (below), as client.dll's `rocket_launcher` smart HUD draws nothing
    else.
  - Sprinting lowers the gun; after it stops, `raise_time` passes before it can fire.
    `zoom_time_out` is used when leaving ADS.
- **Maps:** every multiplayer map loads.
  - DLC maps ship only as `name(NN).rpak`; the first one that isn't a patch is used.
  - Pick the map in the main menu; the game relaunches into it.
  - The 3D skybox is drawn by its own camera, as in Source, so skybox geometry never covers the map.
- **Pilot movement:**
  - All values come from `pilot_mp.set` (over `pilot_base.set`), the settings every
    multiplayer Pilot class uses (the class files only add models and footsteps), as in a
    Northstar match: walk `stand.speed` 173.5, sprint `stand.sprintspeed` 260, crouch 80,
    `gravityscale` 0.8, `jumpheight` 60, superjump 60 / 180, wall-running at up to
    `wallrunMaxSpeedHorizontal` 420 with `wallrunAccelerateHorizontal` 1500 and
    `wallrunJumpInputDirSpeed` 80, crouch view height 32. `PoseMods` blocks (coliseum's speeds)
    only apply with their mod and are skipped.
  - Ground movement is Respawn's WalkMove (client.dll 0x18020cc30), as for BT: Friction
    (0x1801f8090) keeps the velocity along the input up to the wish speed and takes the rest away
    linearly at the stance deceleration, which the pilot settings leave at -1, meaning
    acceleration x 0.6 = 1500 u/s^2 (GetDeceleration 0x1801fcf10); Accelerate (0x1801f0ac0) adds
    up to 2500 u/s^2 along the input. sv_friction and sv_stopspeed aren't used. So a sprint stops in
    0.17 s over 22 units, top speed is reached in about 0.1 s, and turning keeps the speed.
    When the input pushes into the surface the previous move ran into (the move trace's plane,
    which WalkMove and AirMove keep at player+0x2b70 when the blocked handler didn't get
    through: 0x18020e226, 0x1801f25f9), Friction keeps the velocity along the input turned onto
    that surface (plus 1/128 of its normal, 0x1801f81bf), so running into a wall slides along it
    at the wish speed's share along the wall (260 x cos 66 = 106 u/s at 66 degrees; below about
    53 degrees the acceleration's share beats the braking and the slide keeps the sprint speed).
    Without it the slide was braked down to about 10 u/s. BT's WalkMove is the same code, with
    the wall his collision pushed him off last.
  - Collision and steps follow client.dll's movement code. The Pilot is a box (`pilot_base.set`:
    -16 -16 0 to 16 16 72, 47 high crouched), swept against the world's triangles. On the
    ground the move is turned along the floor (0x18020dd06) and traced; a blocked move goes to
    HandleBlocked (0x1802055b0), which slides along walkable slopes (normal z above 0.7) and
    otherwise runs StepMove (0x1801fb0e0): try the plain move, try it again lifted by
    `stepheight` 18 and dropped back down, and keep the lifted one when it went further and
    lands on walkable ground. A steep face under the landing (a bevelled curb edge) still counts
    when it is shorter than `stepTestDist` 10 (the probe goes on 10 units and needs a floor at
    about the same height). The plain move is TryPlayerMove (0x1801fa310: up to 8 bumps, clipping
    along up to 2 planes and their crease, stopping in corners sharper than 30 degrees).
    StayOnGround (0x180207860) then keeps the Pilot on stairs and slopes going down (up to 18).
    In the air, hitting a ledge also tries StepMove, so a jump that rises past a lip lands on
    it. So anything up to 18 units is run over at full speed, and taller ledges stop you at the
    face. Wall-running is the same move with the wall as the floor: the Pilot's up vector
    (player+0x2494, which StepMove and HandleBlocked measure "up" and "walkable" from; the
    wall-run turns it to the wall's normal, 0x1802068c0, and IsWallRunning 0x1802c9350 is just
    up.z < 1) is the wall's normal, so StepMove lifts the hull 18 out from the wall, moves and
    drops it back. Trims, window frames, pillars and clip boxes up to 18 out from the wall are
    run past at full speed (on Angel City a 1.4-unit player clip box stopped runs dead), deeper
    ones stop the run. While wall-running StayOnGround doesn't move the Pilot (0x1802078b5).
    `tf-sim/tests/wallrun_ledges.rs` checks trims 2-24 out, flutes, a pillar, window frames and
    a cornice; `pilotscan <dump> rec <TF_RECORD log> t0 t1` re-runs part of a play session. Automantle on the ground needs jump held (0x180209ed0 checks `IN_JUMP` there and for
    0.25 s after leaving the ground); before this, running into any knee-high ledge started a
    1 s climb. Not modelled or guessed: the slow-fall case's flag (0x2b7c) is ignored, the
    `boostEnabled` air step-up doubling (boost Pilot classes only) is left out, the step's
    view offset is our camera's own smoothing, ties between a triangle's face and its edges are
    resolved toward the face, and getting unstuck from inside geometry (small nudges) is ours.
    The game finds no floor in quadrants as Source's TryTouchGroundInQuadrants does, and
    neither do we.
    `tf-sim/tests/step_sweep.rs` checks ledges, curbs, stairs, ramps, a wall slide, an air step
    and staying crouched under a low ceiling. `TF_COLLISION_DUMP=<file>` makes the viewer save
    the map's collision when it quits; `cargo run --release -p tf-sim --example pilotscan` runs
    the Pilot over a saved dump (sprints in all directions from many spots, reporting stops,
    hitches and ground mantles; `compare` runs the same sprints over two dumps, `replays` reruns
    listed spots, `walls` finds wall-slide test spots, `uncovered` lists solid triangles the
    map's own collision lacks), and `tf-assets --example cmdump` writes the map's own
    `CM_PRIMITIVES` collision in the same format for comparison. `tf-assets --example propcoll` prints each prop model's collision
    data (CM contents, hulls, the per-triangle mesh against LOD 0).
  - Static props collide as engine.dll traces them. Only the props the map lists in
    `CM_PRIMITIVES` (type 3, by static prop index: 11656 of Angel City's 19458) are traced at
    all, each with its contents there (the handler 0x1801288e0 calls the static prop trace
    0x1801b7cf0). That trace has two shapes per model. Traces at TRACEDETAILLEVEL_HIGH, which
    is bullets (ConVar `pertrianglecollision` 1: "Enables per-triangle collision with
    TRACEDETAILLEVEL_HIGH (i.e., bullets) on static models"), go to studiorender.dll's
    per-triangle BVH (v53 header 0x198, walked by 0x18000e570), which is LOD 0's render
    triangles (exactly so for 839 of Angel City's 854 prop models; trees and plants leave their
    leaf cards out). Every other trace, the player's hull traces and wall-run probes included
    (client.dll's Ray_t init 0x1800abec0 leaves the level at 0), sweeps against the model's
    convex collision hulls (header 0x1CC / 0x1D0, 0x18014a1a0 / 0x1801491e0: face planes, then
    bevel planes for box sweeps). So the Pilot and BT hit the hulls (in the movement-only clip
    sets: `CLIP_PILOT` for props with PLAYER contents, `CLIP_TITAN` for TITAN, so BT walks
    through small props and railings), and shots and other sight traces hit the per-triangle
    mesh (the collision world's `detail` set, which movement queries skip). The Pilot's wall-run
    probes and mantle checks use the movement view too, as the game's take the player's own
    mask (0x18020e3cd), so they now also see player clips. Models without hulls (3291 instances
    on Angel City: rugs and floor mats, posted papers, wires, bottles, manhole covers) stop
    shots only. Before this, every prop's render triangles (9.8 million on Angel City) stopped
    movement, so bolts, trims and sandbag folds stopped the Pilot. The models' embedded `.phy`
    (vphysics) data is not what these traces use.
  - Jumps take off at sqrt(2 x sv_gravity x height) (jump handler 0x180200cf9) while gravity in
    flight is sv_gravity x gravityscale (0x1801f74b3): a 60-unit jump takes off at 300 u/s. As in
    Source, each airborne step takes half its gravity before the move and half after, and the
    jump is added to a vertical speed that already lost the first half, then finishes that
    tick's gravity itself (the jump handler's common exit, 0x1802010d6), so the jump step ends
    1.5 steps of gravity under 300: 293.7 at 141 fps, and the jump peaks 73.9 units up after
    0.99 s, as a capture of the real game shows (below). The double jump (0x1802007ba) replaces the vertical speed with a
    full superjump unless rising faster than a quarter-height jump would add, in which case it adds
    that quarter (`superjump_min_height_fraction` 0.25); horizontally it adds 180 along the input
    and takes as much off the old velocity, keeping its direction (0x180200f7c).
  - The keyboard grace (AirMove 0x1801f1d62): for `jump_keyboardgraceperiodmax` 0.5 s after any
    jump (ground, double, wall), a movement key going down turns the horizontal velocity toward
    the held keys' direction at the same speed, by `jump_keyboardgrace_strength` 0.7 until
    `jump_keyboardgraceperiodmin` 0.2 s and fading out by 0.5 s, the change capped at
    `jump_keyboardgrace_max` 0.7 x the sprint speed. It happens once, on the tick of the press:
    the test is on the move data's pressed buttons (`[mv+0x20] & 0x618`, forward, back, left,
    right), which earlier notes read as the held keys, so it turned the velocity every tick and
    a back press flipped a jump around. The capture: back pressed 0.2 s into a 260 u/s jump takes
    182 off on that frame, then air acceleration brakes the rest through zero. Not modelled: the
    game takes the keys along the view's pitched forward (we flatten it), and only buttons count
    there (here a stick leaving the center counts as a press).
  - A capture of the real game is the reference for all of this: the TD.MoveRef Northstar mod
    ran 31 scripted sequences (run, sprint, walk, crouch, back, strafe, diagonals, reversing,
    snap and smooth turns, jumps with air strafing, air braking and air turns, double jumps at
    two timings, a bunny hop chain, slides, slide jumps and slide hops) as pilot_grapple_male on
    mp_forwardbase_kodai and logged the predicted player every client frame. The real game runs
    one movement step per client frame with that frame's time (the server's states equal the
    client's frames bit for bit; the 60 Hz server tick doesn't enter into it), so the viewer
    now steps the Pilot once per frame too (splitting only frames over 1/30 s) instead of in
    1/120 s substeps. `cargo run --release -p tf-sim --example moveref <kodai dump> <replay>`
    replays a sequence's inputs (per frame, from the capture) over a Kodai collision dump, and
    ours matches the capture within its timing noise on every ground number (top speeds,
    2500 u/s^2 starts, 1500 u/s^2 stops, turns), jumps, air control, double jumps, hops and
    slide curves; `tf-sim/tests/capture_ref.rs` checks the key numbers at the capture's 141 fps.
    The capture can't confirm the slide jump itself (every scripted slide jump let go of crouch
    on the jump's frame, which loses the jump).
  - The real game's FOV is `cl_fovScale` x 70 (horizontal at 4:3, clamped to 1.7 by client.dll
    0x1802c59f0); set the same scale in our settings menu to compare side by side.
  - What the movement keys mean comes from their help strings in the game's `client.dll` (the
    engine describes each player setting and convar). Where the text says what a key does, the
    sim does that; the exact formulas behind the text are engine code, so the shapes below are
    our reading of it.
  - Slides follow client.dll's slide code (StartSlide 0x18020af70, WalkMove 0x18020d9f9,
    Friction 0x1801f8090, the jump handler 0x180200609, Duck 0x1801f5500), with the engine's
    player-settings defaults where `pilot_solo.set`/`pilot_base.set` don't set them. A slide
    starts when crouch goes down on the ground (or on landing with crouch held) at a horizontal
    speed of at least `slideRequiredStartSpeed` 200, with the movement input within
    `slide_max_angle_dot` 0.6 of the view's forward: no input, a strafe or a backpedal just
    crouches; where the Pilot is moving doesn't matter. If the last slide started more than
    `slide_boost_cooldown` 2 s ago it adds `slideSpeedBoost` 150, not past
    `slideSpeedBoostCap` 400, and allows one long jump. While sliding, the part of the input
    along the slide is dropped (input only steers across it, at `slideAccel` 50 without gaining
    speed, or brakes against it); speed drops by `slidedecel` 50 per second, or
    `slideWantToStopDecel` 400 when crouch is let go or the input pushes against the slide, and
    then keeps `slidevelocitydecay` 0.7 of itself per second. The slide ends below
    `slideStopSpeed` 125, or when crouch is let go at or below `slideMaxStopSpeed` 350. A jump
    out of a slide is `slideJumpHeight` 50 high and keeps the slide's speed; the long jump
    raises a speed within 85% of `slideMaxJumpSpeed` 350 to 350, and jumping before the Pilot
    is fully ducked (within 0.4 s of the slide's start) takes the boost back (jump handler
    0x180200655: `m_duckState` not 2). Letting go of crouch starts the unduck (`m_duckState` 3,
    200 ms from fully ducked), during which jumps are lost unless still sliding (0x1802003f4): in
    the capture, crouch let go and jump pressed on the same frame of a slide below 350 doesn't
    jump. Ducked in the air (crouch held for Duck's 400 ms), the hull turns crouched with its
    middle kept, so the feet rise 12.5 units (FinishDuck 0x1801f7290) and the jump lands later;
    standing back up in the air lowers them again if there is room. A landing while ducking
    starts the slide on the landing tick itself (SetGroundEntity 0x1801fa1c6). The duck carries
    on in the air, so crouch held through a
    jump lands fully ducked: landing into a slide adds the boost (SetGroundEntity 0x1801fa1c6)
    and hopping straight out keeps it. Every slide start restarts the cooldown, boosted or not
    (0x18020b213), so a slide-hop chain gets one boost and then carries it, losing only the
    slide friction of each landing's ground ticks; `tests/slide_hop.rs` prints the speeds. Sliding up a step costs
    `slide_step_velocity_reduction` 10 per unit of step height (telling a step from a ramp by a
    flat top is ours). The view goes down into a crouch over 0.4 s (0.2 s when the crouch
    starts a slide) and back up over 0.2 s, eased with Duck's smoothstep. Slides tilt the view
    as `CalcViewRoll` does: a tilt vector (the velocity turned 90 degrees left, scaled from 0 at
    `slideStopSpeed` to 1 at `slide_viewTiltPlayerSpeed` 400) moves toward its target at
    `slide_viewTiltIncreaseSpeed` 5 / `DecreaseSpeed` 2.5 per second, and the roll is
    `slide_viewTiltSide` 15 degrees times its dot with the view's forward, so a straight slide
    doesn't tilt and looking off to the side of it does. `slideFOVScale` 1.1 is lerped in and out
    over `slideFOVLerpInTime`/`OutTime` 0.25 s (linear: our reading of the keys). Sounds: the
    crouch-down and stand-up sounds (`sound_standToCrouch`/`sound_crouchToStand`) and
    `jumpjet_slide_start_1p` at the start. Not verified or modelled: the wish speed the slide
    steers with (we use the crouch speed; it only matters for tiny inputs), the slide loop sound
    (`sound_slide_prefix` "pilotslide" by surface), the hull staying standing until the crouch
    finishes, and the slide effects table.
  - Jumps: a jump within `skip_time` 0.1 s of landing is a "skip": `skip_jump_height_fraction`
    0.75 of the height, keeping its speed less `skip_speed_reduce` 12, which doesn't cut below
    `skip_speed_retain` 450 (client.dll 0x1802009b6 takes the speed at the jump: friction since
    landing stays lost). `jump_graceperiod` 0.2 s after walking off a ledge still gives a normal jump. A
    double jump while already rising fast still adds `superjump_min_height_fraction` 0.25 of its
    height.
  - Wall-running is ground movement on the wall's plane, as the keys' text describes:
    `wallrunFriction` 4 ("Base friction applied while wall running", applied Source-style to the
    speed along and up the wall, so entry speed above `wallrunMaxSpeedHorizontal` bleeds off),
    acceleration along the wall from forward input, and up it from `wallrun_upwardAutoPush` 0.65
    ("automatic up-the-wall input applied when the player pushes forward along the wall"; we
    scale it by `wallrunMaxSpeedVertical`), against gravity that "starts at 0 and ramps up to
    full" over `wallrun_gravityRampUpTime` 1 s. At `wallrun_slipstarttime` 1.5 s the run slips:
    it loses `wallrun_slipslowdown` 50% of its velocity, gets a `wallrun_slipvel` 70 push down,
    and turns "completely gravity based" over `wallrun_slipduration` 1 s; with no input at all at
    least `wallrun_noInputSlipFrac` 0.7 of the slip applies. Falling off (time limit, crouch, the
    knee-height probe losing the wall, or pushing away from it, input more than 0.7 along the
    wall's normal, for `wallrun_pushAwayFallOffTime` 0.05 s) adds `wallrun_fallAwaySpeed` 70
    away from the wall.
  - Starting a wall-run follows client.dll (AirMove 0x1801f2628, TryStartWallrun 0x18020b390,
    CanWallrun 0x180202090). It starts only when the airborne move runs into the wall (no
    proximity sense), with no speed, input or view requirement: the velocity just needs some
    component into the wall (`wallrun_minAngle_air` 180), the surface's normal z must be between
    -0.35 and 0.7, and two probes, at knee height (+16) and mid-hull, each a cube of the hull's
    half-width, must find the wall within `wallrun_allowed_wall_dist` 13 of the hull's face.
    When only the probes failed, it is tried once more `wallrun_retry_interval` 0.07 s later if
    the Pilot is still at the wall's plane. There is no wall-run for 0.5 s after leaving the
    ground without jumping or after falling off a wall (a jump has no lockout). Pushing and
    looking squarely into a wall (`automantle_maxangle_push`/`_view` 50 degrees) needs a third,
    head-height probe to find the wall too, so walls short enough to mantle aren't run. Starting
    adds `wallrun_upWallBoost` 250 along the wall's up direction, topping up what is already
    going that way (not capped). The wall last left (normals within `wallrun_sameWallDot` 0.9,
    remembered until landing) can be run again only below where that run started
    (`wallrun_sameWallHeight` 0), and gets no boost; the boost is also skipped after landing
    or falling off a wall until the next jump. The probes are rays (a 3x3 grid) rather than the
    game's box traces, and the wall's normal comes from the hull's first hit. `tf-sim/tests/wallrun_sweep.rs`
    jumps at a wall from 1080 combinations of angle, speed, distance, view and input and checks
    that every jump that reaches the wall in the air starts a run. Curved walls: the run
    turns with the wall the probe finds, at most `wallrun_rotateMaxRate` 3 rad/s, and lets go when the
    wall turns away by more than `wallrun_angleChangeMinCos` 0.8 (37 degrees; a 45-degree
    chamfer drops you onto the chamfer as a new wall). The `func_wallrun_curve` hint surfaces
    aren't read. Not modelled: the wall-jump repel
    (`wallrun_repel*`), `wallrun_avoid_wall_top_decel`, wall-hang slipping
    (`wallrun_hangslip*`, `wallrun_hangStopTime`) and `player_extraairaccelleration`.
  - Air control is Source-style (engine defaults `airSpeed 60`, `airAcceleration 500`). Already
    faster than `airSpeed` along the input, the velocity turns toward it at
    `player_extraairaccelleration` 2 u/s^2, capped at the old speed counted in 3D (AirAccelerate
    0x1801f19a0), so holding forward in a jump trades a little vertical speed for horizontal: the
    capture's jumps gain 0.5-1 u/s across, and a standing jump steered forward lands at 61.
    Automantle
    takes `automantle_duration_*` 1.11/1.0/0.5/0.35 s depending on how far the eye is above the
    ledge (`automantle_height_below/level/above` -10/10/30). In first person the gun goes away
    and the arms play the matching `ptpov_WallMantle_Below/Level/Above/HighAbove` (or its
    `_Crouch` form; `pov_pete_core.mdl`, included by the arms) stretched over that time, with
    their camera bone on the eye; the gun then comes back with its raise (`ACT_VM_RAISE`). The
    view follows our mantle path (up, then over the edge), not the clip's camera, and hiding the
    gun at once (no lowering) is a guess.
  - Sprint speed follows the ground acceleration. `sprintStartDelay` (0.2 s),
    `sprintStartDuration` (0.8 s), `sprintStartFastDuration` (0.2 s, "after landing from a
    sprinting jump") and `sprintEndDuration` (0.15 s) time the sprint view offset
    (`sprintViewOffset` -6), as their help strings say, not the speed; the smoothstep curve over
    those times is a guess. Looking down slides the view forward by up to `pitchOffsetScale` 9.2
    (sin of the pitch, a guess).
  - Aiming down sights scales walking (and BT's) speed by the gun's `ads_move_speed_scale`,
    in proportion to how far the zoom has gone.
  - Camera feel from the set file: smoothed step-ups, a landing dip between
    `viewkickFallDistMin` and `viewkickFallDistMax`, the sprint view offset and lean,
    and slide FOV. There is no camera head bob (the game's settings have none; walking bobs
    only the viewmodel, from the weapon's `bob_*` keys).
  - Movement sounds: footsteps, jump jets, landings, wall-runs, slides and mantles.
  - The jump jets' body tails (`Jumpjet_Jump_Body_1P`, `Jumpjet_Jet_Body_1P` for a double jump and `Jumpjet_Wallrun_Body_1P`) play once from the jump or wall-run start and are cut off on landing or leaving the wall. Their records don't loop and have no end sound, so cutting them at the end of the jump is our reading of how the engine uses them.
  - Ziplines (Rise, Relic and several campaign maps): each `move_rope` with `Zipline` 1 in
    `_script.ent` runs to its `NextKey`, sagging `ZiplineSagHeight` at the middle (a parabola, a
    guess). Use within `zipline_use_range` (120) grabs one, and so does jumping into one (within
    40 units of the top of the hull, a guess). The Pilot rides the way they face, accelerating
    at `ziplineAcceleration` 400 to `ziplineSpeed` 600 over `mountZiplineTime` 0.5 s of blend
    onto the line, hanging with the eye 28 units under it (a guess), and lets go
    `ZiplineAutoDetachDistance` before the end. Jump adds `ziplineJumpOffSpeed` 400 upward,
    crouch drops off, and `useZiplineCooldown` (1 s) passes before the next grab. The gun plays
    `ptpov_zipline_start` on mounting, with the `Player_Zipline_Attach/Loop/Detach` sounds. The
    cables are plain dark tubes, 3 units thick (the game's `cable/zipline` rope material isn't
    loaded).
- **Pilot melee** (`melee_pilot_emptyhanded.txt`, SP values): the gun goes and the Pilot's
  arms play one of `ptpov_emptyhand.mdl`'s ACT_VM_MELEE_ATTACK1 swings (hooks, uppercut, knee,
  air kick) with their sounds, lunging over `melee_lunge_time` at the nearest grunt within
  `melee_lunge_target_range` and `_angle`; the strike lands at the sequence's AE_MELEE_ATTACK
  (0.2 s) for `melee_damage` with the script's view kick. The swing is cut at 0.5 s and the
  gun plays `raise_frommelee_seq` over `melee_raise_recovery_animtime_normal`, or `_quick`
  after a hit. No firing meanwhile.
- **Ordnance throws** (and the thrown tacticals, Pulse Blade and A-Wall, with their own
  timings and viewmodels): pressing the throw key pulls the grenade out (`toss_pullout_time`) on
  its script's viewmodel in place of the gun, holding the key keeps it ready (a fused grenade's
  fuse burns while held and goes off in hand at zero), and releasing tosses it (`toss_time`,
  the projectile leaves at the start of the toss); then the gun plays `raise_seq` (0.33 s).
  No firing meanwhile.
- **Hit feedback:** hit markers and the game's `hitbeep2`.
- **Weapon feel** (`vmmotion.rs`; the keys are the scripts', how the engine combines them is
  reconstructed from client.dll where noted, otherwise guessed):
  - Recoil uses each weapon's `viewkick_*` keys with the spring its `viewkick_spring` names in
    `scripts/weapons/springs.txt` (hip-fire and ADS constants per axis, blended by zoom). As in
    client.dll's fire kick, a shot's pitch and yaw are the base plus or minus a random amount
    between half the `innerexclude` and half the `random` range, and its roll is plus or minus
    `randomMin`..`randomMax`. Pitch and yaw (not roll) are scaled by the kick ramp
    (`viewkick_scale_min/max`, `valuePerShot`, lerp window, decay delay and rate, first-shot
    scale, `duck_scale`); keys a script leaves out take the engine's defaults (soft scales,
    `vmScale`s and the value keys 0, so with no lerp window the kick is at its max scale). The
    hard part moves the angle at once and the soft part pushes the spring (velocity += 20 x
    soft). The whole kick goes into one weapon punch spring, split by `weaponFraction`
    (blended hip to ADS by the current zoom): the camera (and the shots, which follow it) gets
    pitch and yaw times (1 − fraction) and the whole roll, and the viewmodel gets pitch and yaw
    times fraction times `vmScale` with the roll undone, turning the gun about the eye (client.dll
    adds it to the viewmodel's angles after placing it; turned about the far ADS sway pivot, a
    small kick up swung the whole gun down off the sight). `viewmodel_shake_*` moves the gun along
    its forward, up and right axes by the punch roll over 3 degrees
    (`viewmodelShake_sourceRollRange`), so it buzzes with the roll spring. `viewkick_perm_*`
    stays in the aim. A gun's fire animation (`attack_seq`, the hip and ADS clips blended by
    zoom) fades in over the sequence's fade time (0.05 s). Guesses: duck, move and air scales
    are only partly modelled, and missing springs fall back to Source's 65/9.
  - Viewmodels sway from `sway_*` (turn and move pushes, clamps, gains; `_zoomed` variants in
    ADS, where keys a script leaves out are 0) about the `sway_rotate_attach` attachment
    (`SWAY_ROTATE_ZOOMED` sits far along the barrel, so ADS sway keeps the sight on target).
    A full turn push is 150 degrees/s and a full move push is 173 units/s; both are guesses.
  - `bob_*` bobs the gun while walking (one vertical cycle per `bob_cycle_time`, the sideways
    part and angles at half rate, scaled by speed up to `bob_max_speed`).
  - Aiming plays the viewmodel's `ads_in_seq` (hip pose to ADS pose) and leaving it
    `ads_out_seq`, at the zoom fraction, so `zoom_time_in`/`_out` (and fast-ADS mods) set their
    pace and their own paths show: the R-201 reaches the sight two-thirds of the way in, and its
    gun dips below the hip pose on the way out. (We used to blend straight between the two end
    poses.) A change of direction cross-fades the two over 0.1 s (a guess).
  - The FOV narrows over the script's `ads_fov_zoomfrac_start`..`end` part of the zoom (the
    Kraber's 0.5-0.9: the scope comes up first), else over the whole of it; the smoothstep easing
    is a guess.
  - `sprint_seq` loops every `sprintcycle_time` (0.55 s for every gun, while the clips are
    0.667 s long).
  - A gun's first draw plays `drawfirst_seq` over `deployfirst_time` (the R-201's 1.25 s
    charging-handle pull); later draws play `draw_seq` over `deploy_time`. The gun can fire once
    that time has passed, and putting it away takes `holster_time`. (Guns used to fire at 60% of
    the draw, with holsters capped at 0.6 s.) Readiness follows these script times rather than
    the sequences' `AE_WPN_READYTOFIRE` events, which match them only sometimes: at natural
    speed the R-201's first draw reaches its event at 1.27 s (script 1.25 s), but its normal draw
    reaches it at 0.40 s (script 0.7 s).
  - Each activity's sequence is picked by the Pilot's activity modifiers, as the engine picks
    among sequences sharing an activity (each sequence lists modifiers, Respawn's with a negate
    flag; all of a sequence's must hold and the most specific wins). Crouched, and so also
    sliding, the guns play their `crouch` sequences (`ads_in/out_crouch_seq`, `draw_crouch_seq`,
    `reload_crouch_seq`, `raise_crouch_seq`...): the R-201's crouch hip pose cants the gun so
    its sight sits in the middle of the view (its animation is named `ads_in_slide_anim`). In the
    air, `air` ones play (the R-201's air ADS sequences reuse the standing animations; only its
    `sprintraise_seq_air` differs). A change of modifiers cross-fades the old picks into the new
    over the new idle's `fadeintime` (0.2 s). The `ACT_VM_ONEHANDED_*` set is not used: its poses
    drop the free hand to the side, off screen, which looks like the pose for carrying something
    in that hand (the CTF flag; a guess), not wall-running.
  - Weapon-script keys not used yet, because what the engine does with them isn't known:
    `lower_time` (the sprint pose blends in; no `lower_seq` plays), `sway_rotate_attach_blend_time_zoomed`
    (the pivot moves with the zoom fraction instead), `viewkick_move_scale`, `viewkick_hover_scale`,
    `viewpunch_multiplier`, `zoom_angle_shift_pitch/yaw`, `zoom_toggle_fov` (the snipers' zoom
    toggle), `bob_tilt_angle` and the older `sway_turn/move_angle/origin_factor` and `sway_gain`
    keys (the shotguns), and `dof_*` (there's no depth of field). `viewkick_air_scale_ads` is 1 in
    every gun's SP block, so it would change nothing.
  - On top of these, viewmodels play the engine's additive layers: `idle_seq_autoplay` (the
    breathing loop), `walk_seq` over the velocity and `ads_blend` pose parameters in real time,
    `run_layer_reload` when reloading on the move, the jump and land transitions, and
    `sprintraise_seq` when a sprint ends. `TF_VM_NO=walk,idle,jump,land,raise,sway` turns them
    off for comparisons. `walk_seq`'s clips carry constant offsets (the R-201's ADS walk holds
    the gun 2.6 units and 2.3 degrees off the ADS pose, the Wingman's 8.4 units), which pushed
    the sight off centre whenever you moved, so only each clip's motion about its own average
    pose is applied (an approximation of how the engine cancels them).
- **Testing:** `TF_NO_ENEMIES=1` keeps the map empty for movement, weapon and animation tests.
  `TF_PILOT_TP="x y z yaw"` lands the Pilot at a spot after disembarking (e.g. `300 -2600 830 15`
  on Forwardbase Kodai runs along a wall), and `TF_ARMS=<model>` swaps the first-person arms.
- **Cockpit boot:** after embarking, the view comes up from dark as the game's
  `ServerCallback_TitanCockpitBoot` does (exposure compensation -6 stops, released after 0.1 s
  to auto-exposure); without auto-exposure the recovery is an ease-out over about a second.
- **First-person effects in ADS:** `_FP` systems (muzzle flashes, shell ejects) are emitted at
  viewmodel attachments but drawn through the world camera. They're moved to where the viewmodel
  lens shows that point, with the depth stretched by tan(world fov/2) / tan(viewmodel fov/2) so
  they keep their on-screen size too; zoomed in, the world lens had magnified them into a cloud
  over the sights. Pilot tracers start where the muzzle is seen (`vmcam::to_world`, as Source's
  `FormatViewModelAttachment`).
- **Cockpit damage light:** losing a health segment pulses a red light in BT's cockpit for 3 s,
  and doomed, a dimmer red pulses until the end (cl_titan_cockpit.nut `FlashCockpitLight`:
  colours (1, 0.06, 0) / (0.6, 0.06, 0), radius 70, pulse 0.5-1.5 at rate 3). The script's
  `SCR_CL_BL` attachment isn't on this cockpit model, so the light sits at `SCR_BL_BL`; it lights
  only the first-person layer, and its brightness is by eye.
- **Cockpit sparks:** hull damage throws `xo_cockpit_spark_01` from BT's six `FX_*_PANEL`
  attachments, three per thousand-health mark crossed (none for the first hit from full) and 20 on
  becoming doomed (`CalSparkCountForHit`). Cockpit effects are drawn by the viewmodel camera
  (`pfx::emit_named_cockpit`, as `EffectSetIsWithCockpit`) so the cockpit model doesn't hide them.
  `TF_DOOM_BT=<seconds>` dooms BT after that much play.
- **Lost health flash:** the span of BT's health bar a hit took flashes orange-red, holds
  0.15 s and fades over 0.5 s (`ajax_cockpit_lost_health_segment` gets old/new health
  fractions; its look is compiled, so colour and timing are by eye). `TF_HURT_BT=1` deals 1500
  every 2 s in BT for testing.
- **Cockpit jolt:** any hit on BT (hull or shield) shoves the cockpit away from the damage and
  rolls it away from the side the hit came from, with severity damage / 2000, clamped to 1
  (cl_titan_cockpit.nut `CalcJoltMagnitude`). The engine's `CockpitJolt` is compiled, so this kicks
  the cockpit sway spring: up to `cockpitShake_sourceRollRange` (3 degrees) of roll, and about 4 units
  of push (by eye). A hit with no known source pushes from straight ahead.
- **Pilot damage effects:** each hit while hurt starts `P_health_hex` (orange hexes around the
  screen edge) as a screen effect, at most 10, cleared when health is full again
  (cl_pilot_health_hud.gnut); the red vignette follows `DamageOverlayUpdate`'s ramps (brightening
  from full health to 84%, reddening to 48%). The screen-space placement (see Effects) and the
  vignette's strength are guesses. `TF_HURT_PILOT=1` deals 15 damage every 1.5 s on foot.
- **Dash tilt:** BT's view rolls toward a dash's sideways direction, up to `dodge_viewTiltMax`
  (10 degrees) at `dodge_viewTiltIncreaseSpeed` 5, held `dodge_viewTiltFalloffTime` 0.7 s, back at
  `dodge_viewTiltDecreaseSpeed` 2.5; the cockpit model leans `dodge_cockpitTiltMax` 4 more
  (client.dll convars; the speeds are treated as exponential rates and the direction is a guess).
- **HUD power on/off:** the cockpit HUD (Titan or Pilot) is off while embarking and disembarking
  (`ShouldMainHudBeVisible`) and comes back with `MainHud_TurnOn_RUI`'s animation: it opens wide
  (0.225 s) then tall (0.25-0.75 s) while moving out from 10% of its distance, blinking at the
  script's flicker times; turning off only blinks (0.23 s). The HUD is flat here, so the sphere's
  apparent size is drawn as scale / zoom about the screen centre.
- **Cockpit sway:** BT's cockpit model and the Pilot's HUD lag behind turns and movement with
  the `cockpitSway*` factors: pilot_base.set's for the Pilot (gain 1); titan_buddy.set sets
  none, so BT uses the engine defaults from `client.dll`'s settings table (gain 3, turn -4).
  The target is clamped to `cockpitSwayMin/Max*` (+-0.5 degrees, +-1 unit) before the gain and
  followed on `cockpit_spring_*` (65 / 9), all engine defaults. The input scaling is compiled,
  so a turn of 1 degree/s pushes factor x 0.01 and 1 unit/s of speed factor x 0.003
  (reconstruction, as is clamping before the gain).
- **BT's view and footsteps** (`player.rs` `TitanFeel`, read from titan_buddy.set over
  titan_base.set; the quoted meanings are the settings' help strings in `client.dll`):
  - Footsteps run on the player step timer, not the animation (BT's walk and sprint sequences
    carry no footstep events): `footstepInterval` 700 ms walking, `footstepIntervalSprint` 415 ms
    sprinting, any speed over `footstepMinSpeed` 0 on the ground; the first step comes as soon as
    BT moves (as in Source's `UpdateStepSound`).
  - Each footstep jolts the cockpit, not the view ("Cockpit jolt per footstep":
    `cockpit_stepJolt_origin` 0 0 -5). It kicks the cockpit sway spring so it dips about 5 units;
    the kick size is the same reconstruction as the damage jolt. The view itself doesn't bob.
  - Sprinting slides the view down `sprintViewOffset` -25 after `sprintStartDelay` 0.2 s (the
    settings-table default) over `sprintStartDuration` 1.0 s, and back over `sprintEndDuration`
    0.3 s; back in the air out of a sprint (a dash), `sprintStartFastDuration` 0.2 s applies. The
    view leans into sprinting turns by up to `sprinttiltMaxRoll` 3 degrees with the Pilot's
    `sprinttilt_*` steering. The smoothstep curve over those times is a guess.
  - Looking down slides the view forward up to `pitchOffsetScale` 40 units; following the sine
    of the pitch is a guess. The cockpit and BT's gun move with the view (the gun used to stay
    at the eye, so it slid back when looking down and rose while sprinting).
  - Crouching (hold crouch) uses the crouch stance: `speed` 150, `acceleration` 200,
    `deceleration` 500, the 140-unit hull (BT stays down where there's no room to stand) and
    `viewheight` 125. The eye eases down over 0.4 s and up over 0.2 s (Source's duck times;
    a guess for Titans) on Source's spline, the gun drops by `viewmodelDuckOffset` (-4) with
    it, footsteps come every 500 ms (`footstepInterval` of the crouch stance), and
    `sound_standToCrouch` / `sound_crouchToStand` play. In third person BT blends into the MP
    crouch set (`CQB_CrouchIdle_MP`, `Walk_Crouch_forward_MP` / `_backward_MP`, aiming with
    `MP_crouch_Aim_all`) as the eye goes down.
  - Dashes play `sound_dodge_1p` (`tone_dash_1p` in titan_buddy.set) in the cockpit and the
    `dodgeImpactTable` (`titan_dodge`) where the dash leaves the ground (once at the start: a
    guess); the cockpit shifts toward the dash by up to `dodge_cockpitOffsetMax` 3 units as far
    as the dash tilt has gone (the direction is a guess). Each footstep plays the
    `footstepImpactTable` (`titan_footstep`) dust under alternate feet, 45 units either side
    of the middle (a guess; the foot bones aren't used).
  - Landing kicks the view down by up to `viewkickFallDistMaxScale` 6 degrees between falls of
    `viewkickFallDistMin` 10 and `viewkickFallDistMax` 120 units (compared as impact speeds),
    springing back on BT's `viewPunchSpring` (springs.txt `punch_titan_atlas`, 65 / 15). The
    pitch-down direction and the linear scaling are guesses. Landings past the minimum play the
    footstep family's land sound (`buddy_land_default`); a fall past `hardFallDist` 40 is a heavy
    landing: `buddy_heavyland_generic` and the `titan_landing` impact table's dust.
    `sound_groundImpact` names `Titan.GroundImpact`, which isn't in the sound banks. Which
    events the engine picks and playing the dust only on heavy landings are guesses.
- **Viewmodels** (`vmcam.rs`): guns, the Pilot's first-person arms and BT's cockpit are drawn
  by their own camera on top of the world at the player settings' `viewmodelfov` (70 for
  Pilots, 75 in a Titan), so they keep their size while the world zooms in ADS and never clip
  into walls. The sun and the reflection cubemap light them too. Bevy 0.18 clips at
  `near_clip_plane` as well as `near`, so both are set. While zoomed, world effects at the
  muzzle (tracers, flashes) are not yet moved to where the gun appears. The Pilot's viewmodels
  sit at the view origin, as the engine draws them: they ride with the sprint lowering (6
  units), landing dip, step smoothing, shake and view roll. Before, they stayed at
  the simulated eye, so while sprinting the camera was inside the chest and the arms model's
  torso (a full body) filled the view.
- **Fire rate ramp:** `fire_rate_max` with `_time_speedup` and `_time_cooldown` (the Devotion
  winds from 5 to 15 rounds/s over 1.75 s; the XO-16's accelerator mod); `_use_ads` makes the
  rate follow the zoom. **Damage falloff** has the scripts' third step: past
  `damage_far_distance` damage keeps falling to `damage_very_far_value` at
  `damage_very_far_distance`.
- **Reloads resume:** a reload interrupted by a weapon switch keeps the furthest stage it
  reached (`reload_time_late1..3` / `reloadempty_time_late1..3`: a stage is done once the time
  left is within its value), and the next reload takes that stage's time with its
  `reload_late{N}_seq` animation.
- **Spread** (`spread.rs`): every `spread_*` key by stance (standing, moving, sprinting,
  crouched or sliding, in the air, wall-running, wall-hanging; hip and ADS), with the per-shot
  kick and cap per stance and the decay delay and rate. Moving and sprinting have only hip
  values in the scripts, so ADS uses the standing value there.
- **Crosshairs** (`crosshair.rs`): each script's `RUI_CrosshairData` crosshair (tri, plus,
  sniper, shotgun circle, Wingman brackets, launcher scale...) rebuilt from the RUI atlas's
  crosshair parts, since RUI layouts are compiled code. Pieces sit on a ring as wide as the
  current spread cone through the current lens, and fade out in ADS. Which parts make up
  which crosshair, and their size, are reconstructions.
- **Muzzle flashes and shell ejects:** each script's `fx_muzzle_flash_view` and
  `fx_shell_eject_view` play at the viewmodel's `fx_*_attach` attachments (their frames from
  the MDL), for the Pilot's guns and BT's in the cockpit; BT in third person plays
  `fx_muzzle_flash_world` at the gun in his hand. They are drawn in the world (depth-tested,
  so first-person smoke goes behind walls) and fade by size instead of by nearness: a sprite
  fades as it grows to cover the view, and particle models (casings) are hidden within 30
  units of the eye. At hip fire the viewmodel camera has the world's FOV, so they line up.
  Attachment frames are used as the MDL gives them, so a muzzle brake's vent flames (offset
  along the frame's Z) come out sideways. Hitscan shots draw the script's
  `tracer_effect_first_person` (Pilot, cockpit) or `tracer_effect` from the muzzle.
- **Viewmodel offsets:** `viewmodel_offset_hip` / `viewmodel_offset_ads` (right, forward, up;
  sight mods set their own ADS offset so the optic lines up with the eye) move the viewmodel,
  blended by zoom. Axis order and signs are inferred from the sights (HCOG `0 -2 -.75`: the
  glass sits above the iron line, so negative up lowers the gun).
- **Bodygroups:** each script's `bodygroupN_name/set` picks the sight, scope and screen parts
  of the viewmodel and the world gun. `TF_BODY="part=set,..."` overrides them for testing.
- **Effect fixes for shield models:** Graph Scalar `output op` 2 adds (the A-Wall's 10x deploy
  flash then its steady alpha; it used to set, leaving the wall invisible); "orient model z to
  normal" builds a fixed-roll basis (model X toward world down) instead of a shortest-arc turn,
  which buried the wall facing one way; hex shield materials draw with the game's shader
  (fxuber.rs, see Shield looks; `refract_hex_nml` is a UV distortion map there, not a refraction
  of the scene).
- **Weapon screens:** the .mdl's RUI meshes are parsed (`mdl::RuiMesh`: header 0x128/0x12C,
  parallelogram faces on bones with UVs into the RUI canvas; `ruimeshes` example). Each script's
  enabled `UiDataN` names an RUI and the mesh it draws on. The RUIs are compiled UI programs,
  so only the ammo counters get a stand-in: the rounds in the clip, centred in white on the
  part of the 256-unit canvas the faces show, rendered by a small UI camera into a texture
  (`ruiscreen.rs`). Only meshes whose every face shows the whole region get digits (the SMR's
  ring of faces would chop them). Sights and reticles (`r101_sights`, `hcog_upper`, ...) aren't
  drawn.
  `TF_NO_WEAPON_SCREENS=1` turns them off.
- **The Archer's sight** (`launcherscreen.rs`) is not an RUI but client.dll's VGUI screen
  `vgui_pilot_launcher_screen` (1403x998, `scripts/screens/vgui_pilot_launcher.res`), spanning
  the viewmodel's SCR_BL to SCR_TR attachments on the flip-up sight; the smart ammo HUD's
  `rocket_launcher` type (the class name picks it when `smart_ammo_hud_type` is unset,
  0x1804dc580) runs its update (0x1804dfe80), rebuilt here with the `vgui\hud\sram\*` art from
  common.rpak, rendered into a texture on a quad between the attachments:
  - No lock: "NO TARGET IN RANGE", the tickers along the top, six rocket icons and the small
    targeting circle in blue-white (177 213 227).
  - Locking (lock fraction / 0.9 between 0 and 1): "LOCKING", "SCANNING..." over a random range,
    in orange (238 146 8), tickers hidden; two spinning rings (the ring VMTs' LinearRamp, -375
    and 325 degrees/s) close from three times their 337 px size onto the target and fade in over
    the first fifth.
  - Locked: "LOCKED", the target's AI title ("Ion:") and range ("0040m", 0.0254 m per unit),
    still rings; tickers and circle red (205 72 59), rings, text and icons blinking red 0.15 s /
    white 0.4 s.
  - The circle and rings sit where the eye's ray to a tracked point cuts the panel; the point
    eases (SmoothCD, omega 8) toward the lock point relative to the target, or 500 units ahead.
  - All of it fades in over the last 35% of the zoom.
  - Our choices: the ray is rescaled to the viewmodel FOV so the rings stay over the target
    while the world is zoomed (the game cuts the world-space ray); all six rocket icons show,
    as this game keeps no stockpile; the spin direction of the rings is a guess (the VMTs give
    the rate only as a texture rotation, and TF2 ships those VMTs only in the VPKs).
- **Scopes:** snipers with `bodygroup_ads_scope_name/set` show that part (a funnel with the
  reticle and a glowing ring) in front of the eye from `zoom_scope_frac_start`, and only it
  from `zoom_scope_frac_end`, when the scope model would block the view. The scope sits where
  the gun's ADS animation puts its bone `def_c_scope_ads` (the Kraber's and Longbow's sight
  picture then fills the screen height, with the emitter bars at its sides and the reticle
  lines across it). Showing it from those zoom fractions is our reading of the key names.
  Unlit translucent shaders (reticles) are drawn blended and unlit.

### Look and front end

Everything here comes from the install:

- **Lighting:**
  - The BSP's own baked lightmaps (LIGHTMAP_HEADERS + LIGHTMAP_DATA_SKY) supply the indirect light.
  - The sun, ambient light and fog come from the map's `_env.ent`. Its yaw points *at* the sun,
    which I checked against the baked sun-visibility channel.
  - Props, Titans and the pilot's arms take their indirect light from the map's baked light
    probes (LIGHTPROBES / LIGHTPROBE_REFERENCES). Each probe is an L1 spherical harmonic per
    colour channel, and they're resampled onto a Bevy irradiance volume, so a prop in shade
    is no longer lit like one in the open.
  - Reflections come from the map's baked cubemaps (`cubemaps.hdr.vtf` in the BSP's PAKFILE,
    BC6H, one frame per CUBEMAPS entry). The camera uses the one captured nearest to it.
  - Materials use their gloss (`_gls`/`_exp`, roughness = 1 - gloss) and spec (`_spc`, F0)
    maps.
  - Rendering uses physical exposure, bloom and SSAO, with a Graphics option of High, Medium or Low.
  - Debug toggles: `TF_NO_SPEC`, `TF_NO_PROBES` and `TF_NO_CUBEMAPS` turn these off.
    `TF_PROBE_GAIN` and `TF_CUBEMAP_GAIN` scale them, and `TF_MIRROR` makes every surface a
    mirror for checking cubemap orientation.
- **Sky:** the 3D skybox is the vista model around the map's `sky_camera`, drawn at its `skyscale`.
- **Two-layer materials:** `_bm` world materials blend their second layer by vertex alpha
  through the height mask, in a small extended-material shader.
- **Decals:** decals and blended overlays get a depth bias, so they don't z-fight.
- **UI and fonts:**
  - The UI uses the game's fonts. The `.vfont` files are decoded to Titanfall and Metronic Pro.
  - UI art comes from the `ui.rpak` atlases (`uimg`), with images found by the hash of their RUI path.
  - That art covers the title key art and logo, the Titan HUD pieces, the crosshairs and the wave strip.
- **Screens:**
  - The loading screen shows the map's own load-screen art.
  - The main, pause and game-over menus follow `main.menu`'s layout.
  - Loadout (pilot primary, sidearm, anti-Titan, tactical, ordnance and BT's kit) and mission
    screens open from the main and pause menus.
- **Settings:** saved like the game's own config, as convars and `bind` lines in
  `$XDG_CONFIG_HOME/titanium-dioxide/settings.cfg` (else `~/.config/...`). The ranges follow the
  game's `controls.menu` and `video.menu`. Mouse and ADS sensitivity go from 0 to 20 in steps of
  0.2, and `cl_fovScale` from 1.0 to 1.55 on the 70 degree base FOV. Sliders (volume, sensitivities, FOV) can be clicked or dragged
  to a value; other options cycle on click (right-click goes back). There are invert, volume,
  graphics and key rebinding (Enter on a command, then the new key or mouse button).
- **Controller:** button layouts come from the game's `cfg/gamepad_button_layout_*.cfg`. Look
  speeds, acceleration and response curves come from `cfg/aimassist/looksensitivity*.txt`,
  `accelcurve.txt` and `aimcurve_look_N.txt`. The stick dead zone (0.15) is engine-side and
  estimated.
- **Aim assist (controller only):** ADS pull onto a target's chest uses the weapon's
  `aimassist_adspull_weaponclass` and the radii in `cfg/aimassist/adspull_classes.txt`. The
  slowdown over a target (0.6) is engine-side and estimated.
- Menus can be driven from `--script` with `nav=N` (0 up, 1 down, 2 left, 3 right, 4 select,
  5 back).
- **Music:** the lobby menu track plays in the menus and Frontier Defense's wave score plays in game.

### Executions (`executions.rs`)

- **Titan executions**: melee on a doomed enemy Titan within 280 units and 60 degrees starts the
  synced pair from `_melee_synced_titan` / `titan_executions`: BT's execution for the level
  (`_sp_loadouts.nut`: flip takedown, kickshoot on sewers/skyway, pilot rip on boomtown_end) and
  the victim's matching sequence, both riding their root motion from the victim's origin.
  BT is invulnerable during it (TITAN_EXECUTION_ATTACKER_IS_INVULNERABLE) and the victim dies
  at the end. `TF_EXEC_TEST=1` dooms the first landed enemy Titan and stands it in front of BT.
- **Pilot executions**: melee on a grunt from behind (direction <-1,0,0>, min dot 0.2, 115 units,
  40 degrees; `sh_melee_synced_human.gnut`) plays an attacker/victim pair (the no-weapon and
  kick executions) on the third-person body with the camera on its camera bone; otherwise the
  empty-handed punch from `melee_pilot_emptyhanded.txt` (damage, range, lunge range).

### Rodeo and batteries (`rodeo.rs`)

- **Attach**: landing on a living enemy Titan while airborne (within 180 units of its ride spot,
  `RodeoDistanceIsTooFar`) parents you to its `HIJACK` attachment (the `def_c_spineC` bone,
  `GetRodeoSpotOrigin`). The entrance clip is chosen like `GetRodeoDirection` /
  `GetRodeoDirectionFromAbove` (falling onto it: 40 above the spot, faster than 120 down, looking
  at it; otherwise `fromBelow` and the forward/right dots pick front/back lower, back mid, left or
  right), per chassis (atlas / ogre / stryder, `sh_rodeo_titan_anim.nut` aliases).
- **First person**: the Pilot's arms (`pov_pilot_medium_reaper_m.mdl`) play the `ptpov_rodeo_*`
  clips in the attachment's frame and the view follows their `jx_c_camera` bone (forward is the
  bone's Z, up its Y), blending in from your view over 0.25 s. The weapon viewmodel is hidden and
  guns don't fire while riding.
- **Battery rip**: a Titan that still has its battery gets it pulled
  (`ptpov_rodeo_ride_R_hijack_battery` on the medium chassis, with the Titan's own
  `at_rodeo_ride_R_hijack_battery` while it stands still; ogre/stryder use their `_R_hijack_battery`
  clips and keep walking). At 0.85 of the clip (the `rodeo_battery_rip` event range) the Titan
  loses one health segment (`healthPerSegment` 1500 in the SP settings; a doomed Titan dies),
  then you are thrown off at 450 with 180 up (`RODEO_BATTERY_RIP_PILOT_PUSHED_OFF_*`). Without a
  battery you drop a grenade in the hatch (`ptpov_rodeo_*_grenade_1st`) for the same damage.
  Jump leaves the Titan at 350 along your input and 390 up (0.6 s debounce), until the hijack's
  point of no return (0.585, `RodeoPointOfNoReturn`). A ridden Titan doesn't shoot or punch its rider.
- **Applying it**: the battery shows on the HUD; embarking BT with it gives half a segment of
  health (BT's `healthPerSegment` 1800 x `battery_health_frac` 0.5), full shield and 20% core
  (`battery_core_frac`). `TF_RODEO_TEST=1` stands the first landed enemy Titan in front of you
  (double jump onto it), `TF_BATTERY=1` starts with a battery.
- **Anti-rodeo**: an NPC Titan pops its Electric Smoke on itself once you've been on its back
  for 2 s (one charge; the NPC timing is server-side script that isn't shipped, so the delay is
  an approximation). An enemy's cloud hurts the Pilot (`dpsPilot` 45 after the 1 s
  `damageDelay`, scaled by difficulty) and BT (`dpsTitan` 450) like BT's own hurts them.
- Not done: the third-person rider on the Titan's back,
  rodeoing friendly BT from outside, and the entrance clips' camera skims the Titan's hull
  (the attachment's own tilt is dropped: it faces the Titan's yaw).

### BT on his own (`autotitan.rs`)

While you're on foot BT acts as the auto-Titan: he follows you (walking once you're 600
units off, stopping at 350, sprinting from 1500 - approximations; the campaign's `_ai_titan`
script isn't shipped) or guards a spot: the Titan command key (the Titanfall key while he's
alive) sends him to where you're looking (`PrototypeOrderTitanMove`, MP's `eNPCTitanMode`
FOLLOW / STAY with the `Menu_TitanAIMode_*` sounds) and toggles back to following. He
engages the nearest live enemy he can see within his weapon's `npc_max_range`, turning at
the enemy Titans' 2 rad/s, firing NPC bursts (`npc_min_burst`..`npc_max_burst` rounds,
resting `npc_rest_min`..`npc_rest_max`) with the weapon's own spread and damage through the
normal fire path (reloads when empty; no view kick for you). Enemies nearer to him than to
you fight him instead. He uses no abilities on his own.

### Pilot abilities

On foot, Q uses the tactical ability and G the ordnance; both are chosen in the menu (TACTICAL,
ORDNANCE). Values come from the MP_BASE blocks of the weapon scripts (charges are
`ammo_clip_size / ammo_per_shot`, each recharging in `ammo_per_shot / regen_ammo_refill_rate`) and
the constants in their `.nut` files:

- Stim (`mp_ability_heal`): 3 s of 40% extra speed and doubled health regen; 25 s recharge.
  The screen plays `P_heal` while it runs (sh_stim.gnut), as a screen effect like
  `P_health_hex` (see Effects). Cloak and Phase Shift's first-person looks are engine-side. Cloak
  follows the engine's post shader (`engine_post_ps50` in shaders/fxc, cloak.rs): over the arms
  and gun the scene behind is read at three taps 120 degrees apart (`+1,0`, `-0.5,∓0.866` times
  `cloak_pilotAberrationScale` 0.012, jittered by `cloak_pilotNoiseFactor` 0.25 each frame as
  materialsystem_dx11.dll does at 0x18003af97), weighted by `cloak_pilotTint1/2/3` (0.35, 0.5,
  0.65, normalised), times `cloak_pilotBrightenMul` 1.2 plus `BrightenAdd` 0.02, and pulled toward
  its grey by `cloak_pilotDesaturate` 0.6. Guesses: the engine scales the offset by a mask the
  cloaked models write into the frame's alpha, which isn't traced, so the mask is taken as 1
  over the silhouette and the 0.012 is read as a fraction of the screen width.
  `TF_CLOAK="distance jitter mul add"` to tune, `TF_NO_CLOAK_FX=1` for the old screen tint;
  Phase Shift draws the game's colour correction (`materials/correction/mp_ability_shifter.raw`,
  which sh_phase_shift.gnut registers: a 32x32x32 table that maps each colour by its brightest
  channel onto a pale-blue ramp running white, black, then blue-grey, an inverted ghost world)
  as a full-screen pass after tonemapping on the viewmodel camera (colorcorr.rs, so not over
  the HUD), fading over 0.15 s (a guess), with the script's `P_phase_shift_screen`,
  `P_phase_shift_screen_start` and `P_phase_shift_main` systems (Ronin's Phase Dash gets the
  same, with `P_phase_shift_main_XO`); `TF_NO_PHASE_FX=1` keeps the old blue tint.
- Cloak (`mp_ability_cloak`): 15 s during which enemies only see you within 400 units; 20 s.
- Phase Shift (`mp_ability_shifter`): 2 s of being untouchable and unseen; 15 s.
- Grapple (`mp_ability_grapple`), as `client.dll` does it (CPlayer::Grapple and the grapple
  movement code; the `grapple_*` convars and `pilot_base.set` keys): the hook flies from the eye
  at `grapple_shootVel` 3000 up to `grapple_maxLength` 1100, sticks where it hits, or comes back
  at `grapple_retractVel` 6000 (a miss costs no power). `grapple_pullDelay_human` 0.2 s after it
  sticks the pull starts and takes `grapple_power_required` 50 of 100 power: the speed across the
  cable keeps `grapple_initialSlowFrac` (1 horizontal, 0.4 vertical), `grapple_initialImpulse`
  350 is added toward the hook (up to the ramp's 50), and on the ground it lifts off. Then
  `grapple_accel_human` 1000 pulls toward `grapple_lift` 25 above the hook up to a limit ramping
  from `grapple_speedRampMin_human` 50 to `Max` 800 over 1.5 s; speed across the cable bleeds off
  at `grapple_decel_human` 425 except falling speed and what's along the view; gravity is scaled
  between `grapple_gravityFracMin`/`Max` (0.25/0.7, full when the hook is steeply below);
  steering uses `grapple_airSpeedMax` 420 / `grapple_airAccel` 650. It lets go at the hook (under
  `grapple_detachLengthMin`/`Max`, with the 300 impact hop), when looking away from the hook,
  moving away faster than 500 (200 hop), past the reach, when the cable is blocked, after
  `grapple_detachLowSpeedTime` (1.5 s), `WallTime` (1.2 s) or `GroundTime` (0.7 s) slower than
  250, or on Q again; letting go during the pull costs `grapple_detachSpeedLoss` 300 (not below
  430). Jumping while pulled is a double jump and keeps the hook. Power regenerates at 3/s from
  3 s after the pull started. A cable runs from the left hand to the hook while it's out. Not
  matched: one of the looking-away checks compares against a move-data velocity that wasn't
  identified (it always lets go here), the "push under the cable" gravity term reads the
  forward input as a guess, and there's no hook model, rope wiggle or grapple arm animation.
- Pulse Blade (`mp_weapon_grenade_sonar`): thrown at 3500 u/s, sticks, and marks every enemy
  within 1250 units through walls for 6 s; 100 damage on a direct hit; 25 s.
- Holo Pilot (`mp_ability_holopilot`): a copy of you (your body model and gun) for 10 s, as
  server.dll's CPlayerDecoy does it. It carries on with what you were doing (idle below 65% stick,
  run, strafe, sprint, crouch walk, a 2 s slide, wallrun, or a double jump if made in the air),
  facing your view, moved by its animation's root motion; it stops at walls, falls off ledges at
  1.2x its speed and lands into a run, sprint or slide, and jumps off the end of a wall. Pistols
  play the pistol sequences. Enemies shoot at it when it's closer than you or they can't see
  you; two charges, 12.5 s each. Not modelled: its 3 s dissolve and occasional flicker.
- A-Wall (`mp_weapon_deployable_cover`): thrown (ThrowDeployable: 15 units ahead of the eye, 8°
  above the view at 500, up to 3x looking down, gravity x3), it plants where it lands and puts up
  an arc shield (radius 84, 89 tall, 150°) whose face stands at the projectile, facing the throw
  (DeployAmpedWall). It stops enemy rounds for 15 s or 850 damage; 20 s. Drawn by the game's
  `P_pilot_amped_shield` (`TF_OLD_FX=1`: the old flat arc). Your hitscan rounds leaving through
  its face are amped: they use the weapon's `burn_mod_*` (damage and tracer), as
  CodeCallback_CheckPassThroughAddsMods does; projectiles take its damage and explosion as they
  leave through it.

Ordnance:

- Frag Grenade: bounces, explodes after 3.25 s (200 / 800 vs Titans, 320 radius); two charges.
- Arc Grenade (`mp_weapon_grenade_emp`): bursts on impact (40 / 400, 350 radius) and stops the
  enemies it reaches from shooting for 1.5-2.5 s (the EMP screen-effect durations).
- Firestar (`mp_weapon_thermite_grenade`): sticks and burns 6 s, 30 / 60 every 0.2 s in 120 units
  (first burst x1.2, x2.5 against Titans).
- Gravity Star (`mp_weapon_grenade_gravity`): sticks, pops up after 0.8 s, pulls grunts within 300
  units for 2 s, then explodes (25 / 800).
- Electric Smoke (`mp_weapon_grenade_electric_smoke`): a 5 s cloud dealing 150 / 800 per second
  within 210 units after a 1 s delay.
- Satchel (`mp_weapon_satchel`): thrown at 620, sticks; G again sets off every satchel out
  (125 / 2200, 250 radius); two charges.

Thrown ordnance and the Pulse Blade and A-Wall fly as their script's `projectilemodel` with
its `projectile_trail_effect_0`, nose first: stars spin flat and the rest tumble (the spin rates,
25 and 10 rad/s, are guesses), and a stuck one keeps its last pose. `TF_OLD_FX=1` shows the old
glow instead.

Sticky ordnance sticks to enemies it hits. The on-foot HUD matches the Titan's: the same slot frames, and the Pilot's health in BT's status-bar frame at the bottom centre (five segments of 20). The HUD shows both slots with the game's icons,
charges and cooldowns. Script actions are `tactical` and `throw`; `TF_TACTICAL=N` and
`TF_ORDNANCE=N` pick the starting kit; `TF_PILOT_GOD=1` keeps the Pilot alive for tests and
`TF_BT_GOD=1` keeps BT's health full (his hits still log at `tf_viewer::combat=debug`).
`TF_EXEC_TEST=<class>` (ion, scorch, northstar, ronin, tone, legion) stands that class's Titan
in front of BT instead of the first one.

### BT's loadout kits

Each of BT's eight loadouts (keys 1-8) brings its own defensive (Q), utility (E), ordnance (G)
and core (V) - the game's `+offhand1`, `+offhand2`, `+offhand0` slots, in the HUD's order -
read from the game's `datatable/titan_properties.rpak` (the row whose `primary` is the loadout's
weapon; `tf_assets::datatable` parses RPAK `dtbl` tables, see the `dtbldump` example). The HUD
shows each ability's icon and localised name. Numbers come from the ability scripts (SP values
where the script has them):

| Loadout | Defensive (Q) | Utility (E) | Ordnance (G) | Core (V) |
|---|---|---|---|---|
| Expedition (XO-16) | Vortex Shield | Electric Smoke | Multi-Target Missiles | Burst Core |
| Brute (Quad Rocket) | Vortex Shield | Hover | Multi-Target Missiles | Flight Core |
| Ion (Splitter Rifle) | Vortex Shield (energy) | Laser Tripwire | Laser Shot | Laser Core |
| Scorch (T-203) | Thermal Shield | Slow Trap | Firewall | Flame Core |
| Ronin (Leadwall) | Sword Block | Phase Dash | Arc Wave | Sword Core |
| Tone (40mm) | Particle Wall | Sonar Pulse | Tracker Rockets | Salvo Core |
| Northstar (Plasma Railgun) | Tether Trap | Hover | Cluster Missile | Flight Core |
| Legion (Predator Cannon) | Gun Shield | Ammo Swap | Power Shot | Smart Core |

- **Ion** runs on a shared 1000-point energy pool that refills at 100/s after 0.2 s. Laser Shot
  costs 500 and deals 1500 to Titans. The Vortex drains 120 energy per second while held
  (`shared_energy_charge_cost` 2, taken as per 60 Hz frame). Laser Core is a 6.5 s beam that
  deals 200 per server frame. Both draw the scripts' systems: Laser Shot's
  `P_wpn_hand_laser_beam` from the laser pod's muzzle (where its viewmodel holds it at the end
  of the draw) with the `exp_laser_lite` impact table, and Laser Core's sustained
  `P_wpn_lasercannon_FP` from the chest to the hit point with `P_lasercannon_endglow` there,
  kept running for the whole core (a trail system with a second control point,
  `pfx::PfxBeamEnd`).
- **Waves**: Firewall, Arc Wave and Flame Core follow `WeaponAttackWave`. Each wave lays one
  segment every `wave_step_dist` per server frame, up to `wave_max_count` segments, and stops
  at walls and drops. Each wave hits each enemy once. Values per wave:
  - Arc Wave: 1500, radius 112.
  - Flame Core: three waves 128 units apart, 4000 each, radius 180.
  - Firewall: leaves thermite for 5.2 × 1.75 s, burning 100 per server frame within 60 units.
    As in `CreateThermiteWallSegment`, the first step only notes where the wave is.

  Their effects are the scripts': each Arc Wave segment plays `P_arcwave_exp`; each Firewall
  patch runs `P_wpn_meteor_wall` from itself back to the last patch (control point 1); each
  Flame Core row gets two `P_wpn_meteor_waveCP` from the middle wave out to 238 units either
  side, killed after 3 s. (The script links a row's effects across its three waves; read
  literally in thread order the spans come out ±18 units, so ±238, which covers the outer
  waves plus `FLAME_WALL_MAX_HEIGHT`, is a guess at the intent.) `TF_OLD_FX=1` keeps the
  hand-built bursts.
- **Thermal Shield**: held for up to 3 s. It burns caught rounds instead of throwing them back,
  and deals 200 five times a second to anything within 300 units in front.
- **Sword Block**: you take 30% damage while holding it (15% during Sword Core) and can't fire.
  The sword hits for 625, and for 625 + 1400 during Sword Core (20 s).
- **Tracker Rockets**: need a full lock. Each 40mm hit adds a lock stack (3 = full; stacks fade
  after 10 s). Firing launches 6 homing rockets of 350.
- Shield looks: every shield and wall model (the Vortex dome `fx\vortex_shield_hld.mdl`, the
  Thermal Shield's flame fan, the Particle Wall's shells, Legion's Gun Shield, the A-Wall) draws
  with the game's own unlit uber pixel shader (fxuber.rs, fxuber.wgsl, followed from the
  disassembled `shdr` shaders: e.g. the dome's 4bd6a6bd4aad037a, the wall front's
  c8be5a00373d3dff): scrolled and distorted caustic layers, edge and distance fades, the models'
  own vertex colours and alpha (the VVC data in the .mdl; the dome's rim fades to clear), the
  material's blend, depth and cull states. Shield colour follows the game scripts:
  `GetTriLerpColor` in `_vortex.nut` (full 115,247,255, half 200,128,80, empty 200,80,80, crossing
  at 75% and 95%) goes into control point 1 (`EffectSetControlPointVector`) by the Vortex's charge,
  the Particle Wall's damage (of 1750) and the Gun Shield's (of 2500), and the systems' own
  `Remap Control Point to Vector` turns it into the particle colour. Guesses: the particle's
  colour and alpha are passed to the model as its instance colour (the model renderer's own
  mapping isn't traced), the detail texture's UV slot (no shield uses it with a multiply map),
  and the shader's height fog is left out. `TF_OLD_SHIELDS=1` brings back the old stand-ins
  (a disc with a hex lattice). In the cockpit it sits where the game puts it: on the vortex blocker
  viewmodel's `vortex_center` tag at the end of its draw (105 units ahead, 24 left, 20 down,
  tilted like the hand), sized to the dome model (60 units across). BT's left arm comes from
  the shield's own viewmodel (see Offhand viewmodels below).
- Offhand viewmodels (`offhandvm.rs`): every kit ability whose weapon script names a
  `viewmodel` plays it in the cockpit, with BT's arms bone-merged as on the gun's: the Vortex
  blocker's raised left hand (Ion's and Northstar's Tether Trap use it too), the shoulder rocket
  pods (Multi-Target Missiles, Sonar Pulse, Tracker Rockets, Laser Shot, Cluster Missile, both
  pods for Salvo Core), Scorch's Firewall and Flame Core fist slams, Ronin's Arc Wave swing and
  Sword Block, Burst Core's gun grab and Laser Core's pose (arms hanging out of view; its beam
  now starts at that model's chest `muzzle_flash`, 32 ahead of and 44 below the eye). Using an
  ability plays draw_seq over the script's deploy_time; a held one (Vortex, Thermal Shield,
  Sword Block, the missiles' lock-on, a charging core) then loops charge_seq (a delta over the
  end of the draw when it's stored as one); firing or letting go plays attack_seq (held while
  Laser and Salvo Core fire), then holster_seq over holster_time. An attack stored as a delta
  (the blocker's push, which drops the arm out of view) or a model without a holster ends the
  offhand with the attack. `offhand_keep_primary_in_hand` 0 (the cores, Arc Wave, Sword Block)
  replaces the gun's viewmodel while the offhand plays; with it set, the offhand's animations
  move only the left arm and park the right off screen. Guess: the `empty_handed` models
  (Firewall, Flame Core, Laser Core) keep the primary in hand by script but slam with the gun
  hand, so the gun is hidden while they play and drawn again after. Ordnance fires once it has
  deployed (the script's deploy_time: Firewall 0.15 s, Arc Wave 0.2 s, Laser Shot and Cluster
  Missile 0.3 s), so the Firewall starts where the fist lands; the Multi-Target Missiles fire on
  release and Tracker Rockets on the press (they need a full lock then). Use is read from
  the abilities' state: a held flag, a cooldown jumping up, a utility charge spent, the core
  leaving its build-up.
- **Particle Wall**: 1750 health for 8 s. **Gun Shield**: 2500 health for 6 s. Both stop enemy
  rounds from the front (`targets::update_bolts` tests against `titankit::BoltShields`). The
  Particle Wall is the shape `CreateShieldWithSettings` gives its vortex sphere: the outside of
  a cylinder 180 out from the Titan's feet (less 64), 360 tall and 120 degrees wide about the
  view's heading. `P_xo_shield_wall` stands at the same spot with the same angles (the
  server-side `_particle_wall.gnut` isn't shipped; its code is from the Northstar project's
  copy).
  While the Gun Shield stands the gun is zoomed in by force (`GunShieldThink`'s SetForcedADS,
  so the Predator is down and spun up), and in the cockpit its effect stands at the
  viewmodel's `gun_shield_fp` attachment. Effects kept running on the viewmodel
  (`pfx::PfxVmSpace`: the shield, the barrel spin and glow) go where the viewmodel lens shows
  that point, as the one-shot `_FP` systems do.
- **Cluster Missile**: flies at 3500 u/s for 150 heavy splash, followed by 20 bursts over 5 s
  within 250 units. **Tether Trap**: two tethers per use (SP), thrown at 1000 u/s up 0.3 and right ±0.2
  of the view (the game's `caber_shot_thrown_xl` model, tumbling); they bounce off walls, plant
  on floors (`Wpn_TetherTrap_Land`), arm after 1.5 s, live 60 s, at most 4 out, and the first
  enemy Titan in line of sight within 450 units is pinned; one that is struck directly is
  tethered on the spot.
- **Power Shot**: as `OnWeaponPrimaryAttack_power_shot` sets it up (not while reloading or
  empty): the Predator is zoomed in by force, then charges for the mod's `charge_time` 0.75 s
  (`charge_seq`, `P_predator_barrel_blur_glow_FP` on `fx_barrel`) and fires
  (`power_shot_fire_seq`); ordinary rounds wait meanwhile. Whether the charge waits for the
  zoom (as here) or runs alongside it is engine-side. 2000 to Titans; in close-range ammo it's a 16 degree blast (`CloseRangePowerShot`:
  200 to Pilots falling to 50 from 800 to 1600 units), in long-range ammo a single straight
  round with a 150-unit explosion (`LongRangePowerShot`: 1800 heavy splash). Its effects are
  the mods': `P_muzzleflash_predator_PS(_FP)(_ALT)` at the gun's muzzle (weapons::GunMuzzle),
  `P_weapon_tracers_predator` and the `predator_bullet_ps(_alt)` impact table. **Smart Core**: for 12 s, every Predator round locks onto the
  target nearest the crosshair.
- **Salvo Core**: 20 homing rockets at 12/s, 120 each. **Flight Core**: 6 s of rockets at 12/s
  (200 direct, 200 splash).
- **Utility slot** (the table's `antirodeo` column; cooldown = `ammo_per_shot / regen_ammo_refill_rate`,
  charges = `ammo_clip_size / ammo_per_shot`, length = `fire_duration`):
  - Electric Smoke (20 s): a cloud 240 units ahead (or on BT if that's blocked) that after 1 s
    deals 45/s to infantry and 450/s to Titans between 320 and 375 units (`TitanSmokescreen`).
    The cloud's lifetime is engine-side; 7 s is an approximation. Its effect is
    `P_wpn_smk_electric` at the script's five `fxOffsets`, with the lifetime on control point 1,
    which the system reads as such (smoke 1.25-1.5 times it, arcs stopping after it; the
    Smokescreen script that sets it isn't shipped, so that is inferred from the system).
  - Sonar Pulse (16.7 s): marks everything within 1250 units for 5 s.
  - Phase Dash (12.5 s): 1 s phased - not drawn, no damage taken - launched at 1000 u/s along
    the movement input (+200 up), then `move_slow` 0.6 fading over 0.5 s.
  - Hover (10 s, 2.75 s long): `FlyerHovers` - rise at 225-450 u/s for 0.5 s, easing to 70 u/s
    by 1.25 s, horizontal speed capped at 250, `dodge_speed_slow` 0.65 fading 0.75 s after.
  - Slow Trap (15 s, 2 charges): a canister at 1500 u/s; 1 s after landing its gas (240 units,
    12 s; `P_meteor_Trap_start` and `P_meteor_trap_gas` on the canister) slows whoever stands in it (strength approximated at 0.5). Fire sets it off
    (`OnSlowTrapDamaged`: Firewall thermite, the Flame Core, the Thermal Shield): the cloud
    explodes (`P_meteor_trap_center`) and 12 lines of thermite run out from it (`IgniteTrap`: yaw 30 x i, the last six
    reversed, from 75 units out, the slow trap weapon's `wave_step_dist`/`wave_max_count`),
    each patch a 75-unit meteor tick for the Firewall's lifetime, and each line's
    `P_meteor_trap_burn` runs from its end back to the trap (`FlameOn`). The lines' pitch is dropped
    (they follow the ground anyway); explosive barrels aren't a thing here.
  - Laser Tripwire (10 s): three pylons (900 u/s straight, 1200 to the sides) arm after 1 s and
    for 12 s beam between neighbours (`P_wpn_lasertrip_beam`, `P_wpn_lasertrip_base` on each). An
    enemy touching a beam (8 units either side) destroys the group, as does running out of time
    or a tenth pylon going up (at most 9 stand, the oldest group goes); every pylon destroyed
    blows up for 200 (1500 to heavy armour) within 400 with `P_impact_exp_XLG_metal`. The beam
    height (85 above the landing spot: the 45-unit rise plus a guessed attachment) is a guess.
  - Ammo Swap (2 s): `ToggleAmmoMods` - the Predator Cannon swaps its `LongRangeAmmo` mod in
    or out (ADS spread 0.4, falloff 3000-3250 and zoom 40 against 3.4, 1200-1800) with a full
    clip; the viewmodel plays `ammo_swap_seq`. The crosshair style and cockpit tint don't change.
- **Flight Core** (`PROTO_FlightCore`): the same `FlyerHovers` flight for the core's 6 s
  (`core_duration`) with airSpeed 200 (the Hover ability's is 250; the Northstar passive's 350
  isn't modelled), rockets only after the 1 s takeoff, `dodge_speed_slow` on landing.
- **Sword Block** scales hits by 0.3 (0.15 with the Sword Core) when their origin lies inside
  the 150 degree `TITAN_BLOCK_ANGLE` cone of BT's facing; melee passes through (`BasicBlock_OnDamage`).
- Verified in scripted runs: Tracker Rockets need a 3-stack 40mm lock (6 rockets), the Tether
  Trap pins a Titan that walks into it, the Smart Core steers every shot onto the nearest target
  in its cone, the Holo Pilot draws grunt fire.
- **Not modelled**: the MP mods' charged Arc Wave and Power Shot. The Tether Trap's hold
  time (4 s) is an approximation; the Cluster Missile flies at the script's 3500 and the
  Tracker Rockets at `SmartAmmo_SetMissileSpeed` 1800 (their homing is MTMS-style, not the
  script's 100-200 homing speed ramp).
- **Testing**: `TF_CORE_FULL=1` keeps the core meter full for scripted runs.

### Enemy infantry

Every wave also brings a squad of IMC grunts (`npc_soldier`, two more per wave, up to 12) in
fire teams of three around the spawn spots. They use the SP health (90), the R-201 with its
`npc_*` values and its `fire_sound_2_npc` sound, melee from the AI settings (Pilots only), and
the grunt model's own run, idle, aim and death sequences with the rifle in hand.
`TF_GRUNTS_NEAR=1` spawns them in front of BT for debugging.

### HUD damage direction

Hits on BT or the Pilot put a red arc on a ring around the crosshair, pointing back along the
round's path (or at the Titan that punched you). Each arc fades over 1.6 s, and repeated hits
from the same direction refresh it instead of stacking.

### Loading

Kodai is playable about 10 s after launch (6–15 s on other maps); Angel City's `setup` takes about 6 s
(it was 23-26 s). That comes from:

- VPK entries decompressing their LZHAM parts on every core (a map's ~190 MB `.bsp`: 4.2 s →
  0.3 s), and BSP lumps being read in parallel.
- World batches, static props and actor models building their meshes (MikkTSpace tangents) on
  worker threads; only materials and asset handles are made on the main thread.
- Enemy models and their animation includes being read in parallel before spawning, and parsed
  models being shared between chassis.
- Sounds decoding on every core.
- The rpaks (19 on an MP map with the common patch chains) decompressing and being indexed on
  every core (`GameData::add_rpaks`), in the same order and with the same first-wins rules.
- Material textures (starpak reads, the combined spec/gloss and colour + opacity maps) decoding
  on the same workers that build the world, prop and actor meshes (`convert::prefetch_material`
  into `Cache::prepared`, keyed by every input of the decode); the main thread only adds them
  as assets, in the same order. That was 4 s of Angel City's load.
- The sounds decoding on a thread of their own (own VPK handles) while the map loads.
- Every actor template (BT, cockpit, viewmodel, arms, guns, each enemy class) building its
  asset-free part (model, clips, grids, meshes, textures) on every core while the collision
  hierarchy builds (`actor::prefetch_actors`); spawning then only adds the assets.
- The collision sets' BVHs building at the same time, their top levels split a level at a time
  in parallel (the detail set, the props' render triangles, is 9 million triangles on Angel City),
  then the SAH search trees over their leaves (not for the detail set), the top splits binned on
  every core.
- The probed nav grids (MP maps) casting their cells and edges on every core (7-9 s → 0.5 s on
  Angel City), and the particle textures decoding in parallel.

The world's material batches are made in a fixed (key) order, so a load is reproducible: the
collision triangles (`TF_COLLISION_DUMP`), the BVH, the nav grids and the spawn spots come out
the same on every run (they used to follow a HashMap's per-run order). `RUST_LOG=info` logs
each phase of `setup` as `load phase: <what> <time>`.

Loading takes two frames: `setup_world` builds the game data, world, props, sky and probes and
spawns the cameras and the sun, and `setup` builds the rest (collision hierarchy, BT, viewmodels,
nav grids, enemies) in the next frame, while the render world uploads the map's meshes and
textures (371 MB and 2.2 GB on Angel City) and compiles its shaders. In one frame all of that
waited until the end; Angel City goes from about 9.2-10.5 s to 8.7-10 s from launch to playable
(shared machine, noisy), and the warm-up below from 56-82 frames to 34-40. What is left there is
mostly Bevy composing the ~26 PBR shader variants one at a time (~40 ms each, behind its shader
cache's lock) after the uploads.

The loading screen then stays up while the world warms up behind it (`ui::loading_tick`): every
first-person model is shown (they skip frustum culling, so they count from wherever they sit),
the viewmodel camera already has the reflection cubemap, and the screen waits until no render
pipeline has been compiling for 4 frames (at most 600 frames). Without this the map drew in
pieces and the cockpit appeared about 0.2 s into play. The Titan gun's model info (sway pivots,
muzzle attachments) is also read under the loading screen instead of on the first frame.

### Rendering cost

Static props use the model's own LODs (VTX switch points and VVD fixups, up to three per model,
skipped when a LOD isn't at least 10% lighter). A switch point is about 3.2 x its distance in
units, which is an approximation of Source's screen-size metric. Props whose bounding sphere
would cover less than about 5 pixels stop drawing (150 bounding radii, never closer than 600
units). Props with a radius of 400 units or more are always drawn. Props smaller than 150 units
don't cast real-time shadows, because the lightmaps already hold their shadows. LOD switches and
the draw distance crossfade with a dither over the last 10%. Titanfall 2's static prop lumps
carry no fade distances on the maps checked (-1), so these limits are our own.

On Forwardbase Kodai at 3414x1357 this takes BT's spawn view from about 65 fps to about 90 fps.
`TF_PROP_LOD=0`, `TF_PROP_CULL=0`, `TF_PROP_SHADOWS=all` and `TF_PROP_RADII=N` switch the
parts off for comparisons, and `--features profile` writes a Chrome trace of every system (with
system names).

Small props (the ones that cast no real-time shadows) are drawn from batch meshes
(`propbatch.rs`): one entity per 2048-unit cell, material and LOD instead of one per prop, mesh
part and LOD. Angel City goes from 46 235 prop entities to 4 943 plus 4 157 batches (Bevy runs
its visibility, bounds and specialization passes over every entity every frame, drawn or not).
A batch mesh only lists (prop, model vertex) pairs; its vertex shader (`propbatch.wgsl`) reads
the model's vertices and the prop's transform and draw range from two storage buffers and does
per prop what Bevy does per entity, with the same functions: the draw range test from the
prop's origin, the dithered LOD and distance crossfade, and the vertex transforms.
StandardMaterial's own fragment shaders shade it, and a batch's own range and bounds enclose its
props', so Bevy only culls a batch when all its props would have been culled. Shadow casters,
translucent materials and effect materials keep their own entities. Free-camera screenshots
(7 spots on Angel City, 5 on Kodai) match the per-prop version bit for bit with `TF_NO_UBER=1`,
except ~770 pixels (0.04%) of one Angel City overview on a coplanar strip whose z-fighting winner
depends on draw order (registering the batch material alone flips some of them) and 11 pixels on
Kodai. With the haze cards on, their scrolling follows the time since launch, which shifts with
the (shorter) load time. The collision dump is identical. Angel City on foot at 1920x1080,
uncapped, three interleaved runs each: 85 -> 98 fps average; main thread PostUpdate 7.3 ->
5.7 ms (profile build), GPU +0.15 ms per frame (vertices of out-of-range props in a drawn batch
still run the vertex shader). Loading Bevy's prepass for these meshes logs "The default normal
prepass expects the mesh to have vertex normal attributes" a few times: the batch material
supplies them itself. `TF_PROP_BATCH=0` gives every prop its own entities again.

Materials are bound one bind group per material, not through Bevy's bindless material slabs
(the `BUFFER_BINDING_ARRAY` feature is turned off; lightmaps and light probes don't need it and
stay bindless). With slabs, each switch between material slabs made wgpu-core walk every texture
in the slab (up to 2048) while encoding the pass, 4-6 ms per shadow cascade on Angel City; the
per-material groups draw the same pixels. Angel City, BT's spawn, 1920x1080, uncapped: cockpit
34-53 fps before, 64-81 after; on foot 26-46 before, 56-67 after. `TF_BINDLESS=1` brings the
slabs back for comparisons.

The map's static geometry (world batches, props, probes) hangs under its own root entity, apart
from everything that moves. Bevy skips a hierarchy in which nothing moved, but under one shared
root it recomputed all ~47k static children every frame (main thread: transform propagation
3.7 -> 1.1 ms on Angel City).

The 2D cameras (the HUD and the weapon screens rendered to textures) have no render layers.
Bevy's visibility pass tests every entity against every camera's frustum, and with the default
layer those orthographic boxes, which reach deep into the map, took in ~10k props on Angel City,
marked them visible and sent them to the render world for nothing (the UI doesn't use render
layers or that pass). Angel City at 1920x1080: about 86 -> 89 fps on average (small; within run
noise).

Bevy rebuilds every mesh bind group every frame, and with lightmaps that is one per lightmap
slab (four lightmapped entities) per render phase: ~750 bind groups created and dropped per frame
for Angel City's 419 world batches. `bindcache.rs` keeps the last two sets (the skin buffers
alternate between frames) and reuses one while the instance, skin and morph buffers and the
lightmap slabs are unchanged; a lightmap slot being filled or freed rebuilds for a few frames.
Bevy sized the per-phase instance buffers to exactly the frame's instance count, so every new
high made a new buffer (and a 5 ms rebuild, ~15 times a minute on foot); they now grow by half
again (rebuilds on the Angel City run: 146 -> 8).
Cockpit 64-81 -> 72-93 fps, on foot 56-67 -> 53-80. `TF_NO_BIND_CACHE=1` turns it off.

Bevy draws each shadow cascade in one batch set per material (200-540 per cascade on Angel
City), but an opaque material's shadow pipeline has no fragment shader and never reads the
material. `shadowbatch.rs` queues those meshes in one set per pipeline instead (60-150 sets
remain per cascade, mostly alpha-tested materials, which keep theirs): about 2.3-3.6 -> 0.9-1.4 ms
of CPU per cascade. The depth written is the same (screenshots with time frozen, flipping between
the two every 4 s, match bit for bit). `TF_SHADOW_BY_MATERIAL=1` turns it off.

The render thread walks the render graph one node at a time, and with wgpu 27 a pass's commands
are only validated and encoded when its command encoder is finished. Bevy's opaque, prepass,
shadow and bloom passes are encoded on worker threads, but its transparent pass went into the
graph's own encoder, so every sorted translucent draw was encoded serially. `transparentpass.rs`
puts it in a worker task like the others (same commands, submitted in the same order): the graph
walk went from about 4.0 to 3.1 ms. `TF_SERIAL_TRANSPARENT=1` keeps Bevy's node.

The guns' ammo counters (`ruiscreen.rs`) render through small UI cameras into textures. Those
cameras now render only for 30 frames after a counter's text changes (and until it is laid
out); the texture keeps the last picture. Drawing the same digits every frame cost the render
thread about 1 ms (two cameras' graph, UI preparation and command buffers).

The viewmodel camera keeps only the sun's first shadow cascade (0.1-10 m): Bevy builds all four
for every camera the light reaches, but first-person models are within a few metres of the eye
and never sample the others, so three shadow passes of the whole map per frame were wasted.

The game's own per-frame systems avoid change-detection churn: `animate_actors` reuses an
actor's last pose while its inputs (layers, aim, grids, overlays, offsets) are bit-identical and
writes only bones that moved (of ~5400 bone transforms written per frame, ~2800 were unchanged,
mostly pooled enemies waiting hidden); Visibility is set only when it changes; empty particle
layer meshes are no longer re-marked modified every frame (`Assets::get_mut` alone does that:
~39 mesh re-uploads a frame on foot). BT's mirrored clips and the kit's offhand viewmodels are
built under the loading screen instead of on the first frame of play (a 430-590 ms hitch).
Collision queries reject more work exactly (same results bit for bit): Titan steps ~35% and
Pilot steps ~40% cheaper offline. With wave 3 enemies on Angel City: 58 -> 72 fps average at
1920x1080 (noisy, shared machine).

Enemy moves and actor poses run on every core: each enemy's decisions and movement (steering
and route raycasts, the 120 Hz Titan steps) read only that enemy and the world, so they are
worked out in parallel, and the attacks that follow still run one enemy at a time in the same
order (anything else that depends on an earlier enemy's attack waits for it); the actors whose
pose inputs changed are evaluated in parallel. The HUD writes UI node sizes and positions only
when they change (52 -> 5 nodes re-marked per frame, each sending the HUD through Taffy's
layout). Angel City, wave 3, 1920x1080 profile: the Update schedule 6.0-6.4 -> 3.7-4.4 ms,
`update_enemies` 2.7-3.0 -> 0.9-1.1 ms, `animate_actors` 0.6 -> 0.4 ms, `ui_layout_system`
0.65 -> 0.35 ms; uncapped 66-73 -> 72-83 fps over three alternating runs (noisy, shared
machine). `TF_FIXED_DT` runs (`TF_RECORD` state, hashes of every entity's transforms,
visibility and UI nodes, screenshots) are identical before and after. The particle simulation
(`update_pfx`, 0.5-0.9 ms) stays on one thread: its effects draw from one random stream in
order.
The collision BVH's median splits overlap a lot (a Titan's sphere push walked ~184 nodes to find
~3 triangles that actually touch it), but its leaves and walk order define the query results, so
they stay: queries walk a binned-SAH tree built over those leaves instead and keep the old
results bit for bit. Box queries (sphere pushes, box sweeps) put the leaves they
find back into the old walk's order; rays up to 400 units find every leaf they reach and replay
the old nearest-first walk, with its shrinking limit, over the ones with hits (ties included);
longer rays, and level rays exactly at a leaf's top height (where glam's NaN handling makes the
slab test pass any box), still walk the median tree. Offline on Angel City's collision (one
core): Titan step 47.6 -> 19.8 us, Pilot step 27.4 -> 21.4 us, rays 6.7 -> 5.1 us, sphere
pushes and box sweeps ~10% cheaper. In game, wave 3 on Angel City (`--features profile`):
`update_enemies` 2.4-2.9 -> 1.5-1.9 ms a frame on average. The detail set (the props' render triangles, small and
even, so its median leaves are tight already) keeps walking its median tree: a search tree there
didn't make its queries faster and cost ~0.3 s of load and ~130 MB.

The sky and world cameras don't copy their picture to the window (`CameraOutputMode::Skip`):
the viewmodel camera draws last into the same HDR textures and its copy overwrites the whole
window, so theirs were two wasted full-screen passes (~0.05 ms GPU at 3440x1440, plus their
encoding).

Measuring: `TF_FPS=1` shows the frame rate in the top-left corner (average over half a second,
its frame time and the slowest frame in it), and the log has a `perf:` line every two seconds
with the slowest frame. `TF_NO_VSYNC=1` lifts the monitor's refresh cap. `TF_GPU_TIMES=1` logs
Bevy's render diagnostics every four seconds: CPU and GPU time (timestamp queries) and pipeline
statistics of every render pass. Bevy averages a pass name over the cameras that run it, so
`TF_GPU_TIMES` also logs a `gpu:` line every two seconds with each pass's GPU time per frame
summed over the cameras, split into the cameras' runs (largest first; the sky, world and
viewmodel cameras each run their own opaque pass and tonemapping), and the frame's total.

`TF_FIXED_DT=60` (testing) advances the game by exactly 1/60 s per frame and runs the `--script` and
`--shots` clock on frame counts, so a scripted run plays out the same, frame by frame, whatever the
frame rate: the `TF_RECORD` logs and screenshots of two runs match exactly, which is how changes to
the per-frame systems are checked for unchanged behaviour.

### Campaign maps

The map menu also lists the campaign levels by mission name (all but The Ark, whose ships are
script-moved entities). They have no Titan start point, so BT starts at the densest cluster of
the level's own enemy `npc_titan` placements (from `_script.ent`), and those placements are the
first enemy spawn spots. Levels without Titans use `info_player_start`. Campaign maps also ship
the game's navmeshes (`maps/navmesh/<map>_{large,medium,med_short,small}.nm`: Respawn's tiled
Detour format, decoded in `tf_assets::navmesh`; `navls` prints a mesh's tiles, polys and links);
enemy Titans path on the `_large` one. MP maps have none on the client side.

### Sound

Sound comes straight from the game's Miles banks in `r2/sound`:

- `tf-assets/miles.rs` reads `general.mbnk` and the `.mstr` streams, using the layout from
  LegionPlus.
- `tf-assets/binka.rs` is a Rust port of FFmpeg's Bink Audio decoder. Its output matches FFmpeg's
  to within 3e-5.
- Sources are decoded once and cached, with surround and mono folded down to stereo.
- They play with distance falloff and panning relative to the camera.
- Wired up so far:
  - Every Titan and pilot gun: first shot, randomized repeats and tail; reloads come from
    the animation events below.
  - Missiles: lock, launch and explosions.
  - Vortex: start, end, throw and absorb.
  - BT's footsteps, dash and punch.
  - The Titanfall landing and Titan death explosions.
  - Enemy Titans' weapons, heard in the world.
  - BT's voice lines for core ready/activated and doomed.
- Sound events: `miles.rs` parses the bank's event table and its sound nodes (single sources
  and random containers), so a game event name resolves to the sources it can play.
- Animation sounds: viewmodel sequences carry `AE_CL_PLAYSOUND` events (parsed from the MDL),
  and `actor_event_sounds` plays each one as the animation passes it. Every Titan and pilot
  weapon gets its real reload, draw and holster sounds in time with the animation. 614 of the
  622 events across all viewmodels resolve; the rest are dead names in the shipped models.
- Typed animation sounds: `AE_CL_PLAYSOUND_FOR_TYPE` events get the actor's sound type as a
  prefix, so enemy Titans' walk cycles play their own footsteps and servos at their position
  (for example `Jog.Generic_3P` on a Scorch plays `scorch_jog_generic_3p`).
- Gameplay sounds play as the game's own sound events wherever the scripts name one (weapon,
  ability and pilot scripts). Each event's Miles record supplies its volume, whether it is
  positioned in the world, its falloff distances and whether it loops. Cues whose event the
  bank lacks fall back to hand-mapped source names.
- Stim, Cloak and Phase Shift hold their 1P sustain loops while they run and play their end
  sounds when they stop.
- The mixer has distance falloff and equal-power panning, plus occlusion: a ray through the world
  muffles and low-passes a sound. It allows 48 voices and at most 4 of one world event.
  Explosions duck the music, and dialogue ducks music and effects. These limits are our own;
  the game's are engine-side.
- **Music** follows Frontier Defense's own `music_mp_fd_*` events: the intro for the
  difficulty, the wave-start stinger under the mid-wave bed (`finalwave` every fifth wave),
  the wave-cleared stinger into the between-waves bed, and the defeat.
- **Dialogue:** the FD commander's intro, wave-start, incoming-Titans, wave-victory, defeat and
  Titan-ready lines, plus BT's gameplay lines. They are queued so they never overlap.
- **Subtitles** come from the game's closed captions (`resource/subtitles_english.dat`, a Valve
  VCCD file in the frontend VPK), looked up by the CRC32 of the source name that played.

### Numbers come from the game

Gameplay values are read from the install at runtime, or ported from the game's own Squirrel
scripts (`scripts/vscripts`, which ship as plain text in the VPKs), using the campaign (`solo`
playlist, `[$sp]`) variants:

- **Titan health** (`_titan_health.gnut`):
  - The shield absorbs damage first, waits for `titan_regen_delay` (scaled by hit size,
    `variable_regen_delay`), then refills fully in 2 s.
  - Health never regenerates in the campaign.
  - A killing blow dooms the Titan instead: it gets `healthDoomed` extra health and 0.25 s of
    invulnerability.
  - BT has 9000 health, a 1000 shield and 2500 doomed health (`titan_buddy.set`).
  - Enemy Titans have 8000 health, a 2500 shield and 4000 doomed health (their
    `scripts/aisettings/npc_titan_*` files and the `.set` each one names).
- **Core meter** fills only from Titan damage:
  - Dealing damage gives 0.01% per point, and dooming a Titan gives 10%.
  - Taking damage gives 0.002% per point.
  - Nothing is credited while the core is in use.
- **Pilot health** (`_health_regen.gnut`, `_sp_difficulty.gnut`):
  - Regenerates 40/s after a delay of 0.8 s (healthy) to 3 s (nearly dead).
  - Each enemy hit is capped by the difficulty's `maxDamagePerHit` (60 on Normal) within any
    0.25 s.
- **Difficulty** (`_sp_difficulty.gnut`) sets:
  - the damage multiplier against you (0.5/1/1.5/1.9);
  - enemy Titan proficiency (poor/average/good/perfect), which drops one step while BT is doomed;
  - the aim-cone focus each AI starts with when it first sees you.
- **Enemy classes** each use their aisettings file, MP model and default weapon:
  - Ion (`npc_titan_atlas_stickybomb`, Splitter Rifle), Scorch (`npc_titan_ogre_meteor`, T-203
    Thermite Launcher), Northstar (`npc_titan_stryder_sniper`, Plasma Railgun), Ronin
    (`npc_titan_stryder_leadwall`, Leadwall), Tone (`npc_titan_atlas_tracker`, 40mm Tracker
    Cannon) and Legion (`npc_titan_ogre_minigun`, Predator Cannon).
  - Damage uses the scripts' `npc_damage_*` values where they exist (for example Legion's 150→108
    against Titans). NPCs fire at most once per 10 Hz server frame, which those values compensate
    for ("1 shot per frame max").
  - Thermite and 40mm splash hit BT with `npc_explosion_damage_heavy_armor`.
  - Bolt speed, drop and pellet patterns come from each weapon's `.nut`, as for BT's loadouts.
  - They arrive by Titanfall: each rides its chassis' `at_hotdrop_drop_2knee_turbo` root motion
    for the last two seconds before impact (staggered by up to 1.5 s), lands with the blast and
    dust ring, and stands with `at_hotdrop_quickstand`.
  - Heavy chassis (Scorch, Legion) use their own aim layers (`Aim_Stand_MP`, `Aim_run_MP`).
  - Between bursts each class uses its MP ordnance: Ion's Laser Shot, Scorch's Firewall,
    Northstar's Cluster Missile, Ronin's Arc Wave, Tone's Tracker Rockets and Legion's Power
    Shot. Damage, range, the 8-16 s rest between uses and the 3P fire sound come from each
    weapon script, the Cluster Missile and Tracker Rockets fly at the scripts' 3500 and 1800.
    Scorch's Firewall and Ronin's Arc Wave are the same ground waves as BT's
    (`WeaponAttackWave`: 15 steps of 100 / 112 units, climbing rises up to 0.577 x step and
    dropping up to 1000): the Firewall leaves 5.2 s thermite (`FLAME_WALL_THERMITE_DURATION`,
    no SP scale for NPCs) that ticks BT 100 and the Pilot 20 per server frame, the Arc Wave
    hits BT for 1000 and the Pilot for 50 (`npc_damage_*`) and slows BT (`move_slow` 0.5 for
    2 s). Waves hurt BT, the Pilot on foot, and their owner's enemies only.
  - Each class also has its MP defensive (the NPC rules for using them are server script that
    isn't shipped: here a Titan raises it after 200 recent damage from a target it sees in
    front, Scorch also when you're within his shield's 300 reach). Ion's Vortex Shield (held
    3 s or until nothing has hit it for 1 s, 4 s cooldown) catches up to 32 rounds from a
    120 degree front and throws them back at 35 / 140 each; Scorch's Thermal Shield (3 s, 9 s)
    eats rounds and burns BT 200 and the Pilot 25 five times a second within 300; Ronin's
    Sword Block (3 s, 2 s) takes 0.3 x damage from a 150 degree front; Legion's Gun Shield
    (6 s, 8 s) soaks 2500 from the front. Melee and executions pass through. A Titan holding
    the Vortex or Thermal Shield doesn't fire. Tone drops her Particle Wall (1750 health,
    8 s, 14 s cooldown) around herself, 100 ahead as an NPC's is: BT's and the Pilot's shots, bolts and missiles stop on it
    and wear it down while her rounds pass through. Northstar has none (her defensive is the
    Tether Trap, used as her ordnance here).
- **Enemy Ions** fire the Splitter Rifle (`mp_titanweapon_particle_accelerator`):
  - Bolts travel at 5000 u/s (`TPAC_PROJECTILE_SPEED_NPC`).
  - Bursts are 4-8 rounds at 9/s, with 0.5-1.2 s rests.
  - Damage is 90→60 against Titans and 100→80 against Pilots, plus 100 splash on Pilots.
  - Spread is the weapon spread scaled by proficiency.
  - Against a fast-moving Pilot they deliberately miss half their shots.
  - The Vortex Shield absorbs the bolts but doesn't return them
    (`vortex_refire_behavior "absorb"`, as in the game).
  - They dodge when recent damage passes `StrafeDodgeDamage`, at most twice per 8 s, punch for
    500-600 inside `MeleeRange`, and hold the chase-stop / circle-strafe distances from their AI
    settings.
- **BT's kit** uses the SP weapon values:
  - Multi-Target Missiles: 30/s, 150 direct damage, 100 splash over 200 units, 12 locks.
  - Burst Core: 1.85 s spin-up, 5.5 s duration.
  - XO-16, Vortex and punch: from their weapon files. The Vortex is the script's 150-unit
    sphere, which catches fire arriving within its 120-degree front cone.


### Milestone 4 (done): Pilots

Press X in BT to disembark (BT plays his dismount, you climb out over the hatch and drop from it
with BT's velocity, as the game only unparents you there, landing in embark range) and X near BT to
embark (he kneels while you climb in). On foot you have the R-201 viewmodel with the pilot's arms,
MP pilot movement from `pilot_mp.set` (walk 173.5, jump 60, double jump, slide boost, wall-running
at up to 420 units/s for 1.75 s with wall jumps; the view rolls away from the wall by up to
15 degrees at rate 6, starting 0.25 s before reaching it - `client.dll`'s `wallrun_maxViewTilt`,
`wallrun_viewTiltSpeed` and `wallrun_viewTiltPredictTime` defaults, with the rate treated as
exponential; sprint 260 from `stand.sprintspeed`; fields the file leaves out take the engine's
player-settings defaults read from `client.dll` - ground acceleration 2500, deceleration
2500 x 0.6, airSpeed 60, airAcceleration 500; see `tools/settings_defaults.py` and `tools/cvar_defaults.py`;
the sprint lean follows `sprinttilt_turnrange` 120 deg/s, `sprinttilt_maxvel` 2 and
`sprinttilt_accel` 35, steering guessed), and T calls BT down with a Titanfall at your crosshair (never closer than 600 units). He rides the
root motion of `at_hotdrop_drop_2knee_turbo` down from about 11,000 units up, as
`_titan_hotdrop.gnut` does, and then stands with `at_hotdrop_quickstand`. On impact he deals
`damagedef_titan_fall` to whatever is underneath (23,000 to Titans, ignoring shields and doom) and
`damagedef_titan_hotdrop` around him (150 within 250 units). Embark and disembark last as long as
BT's own sequences.

Titan transitions use BT's own sequences from `titan_buddy_embark.mdl`:
- Embark picks `at_mount_stand_front/behind/left/right` by which side of BT you are on. The
  camera turns to his chest as he reaches for you, then settles into the cockpit.
- After a disembark (`at_dismount_stand`, or `at_dismount_crouch` with the arms'
  `ptpov_dismount_buddy_crouch` when crouch is held, as `GetDisembarkSequenceForTitan` picks
  by `IsStanding`), BT closes up with `at_MP_disembark_back2idle`. The `_fast` set needs the
  MP passive `PAS_FAST_EMBARK` and isn't used.
- X while BT is doomed ejects, as `TitanEjectPlayer` (`sh_titan.gnut`) does. BT plays
  `at_MP_eject_stand_start` for 0.95 s (blendDelay 0.15 plus TITAN_PLAYEREJECT_DURATION 0.8).
  You then launch at 1500-1700 u/s times the square root of gravityscale, 5 degrees back from
  straight up, looking 80 degrees down, and BT self-destructs. Sounds: `titan_eject_xbutton`,
  `Titan_Eject_Servos_3P`, `Titan_Eject_PilotLaunch_3P`, `player_eject_windrush`.
- The automatic eject on destruction uses the same launch.
- BT's sequences play their own sound events (`AE_CL_PLAYSOUND`) at his position: the embark
  Foley for each side, the dismount, `bt_hotdrop_turbo` and the quickstand footsteps.
- `TF_DOOM_BT=1` dooms BT once a game is running, for testing the eject.

Script actions: `interact titanfall jump crouch`.

### Milestone 3 (done): Titan combat

BT carries the XO-16 (stats from `mp_titanweapon_xo16_shorty.txt`: 12 rounds/s, 30-round clip,
2.6 s reload, damage falloff, spread bloom, view kick, ADS zoom), shown as the first-person
viewmodel with BT's arms bone-merged onto it and as the gun in his hand in third person.
Hold Q to paint up to 12 locks with the Multi-Target Missile System and release to launch homing
missiles from his shoulder pods; V fires Burst Core once its meter fills; hold E for the Vortex
Shield (catches incoming fire; bullet-type rounds are thrown back on release); F punches. The
enemies and the health rules are described under "Playing" above.

Test actions for `--script`: `fire ads reload aim ordnance core vortex melee` alongside the
movement ones (`aim` points the view at the nearest living dummy).

### Milestone 2 (done): Titan control

```
cargo run --release -p tf-viewer
```

You are BT at a Titan spawn in Forwardbase Kodai. Click to grab the mouse (Esc releases), WASD to
move, Shift to sprint, Space to dash (two dash pips, recharging), C to switch between third person
and the cockpit, F1 for a free camera. Movement tuning comes from the game's own
`scripts/players/mp/titan_buddy.set` (walk 280, sprint 420, dash 685 for 0.3 s, 50 power per dash;
acceleration 900, or `lowAcceleration` 1500 below `lowSpeed` 200, sprint acceleration 120).
BT blends his walk/sprint animations by direction and speed, and the game's aim matrix layer
follows your view pitch. Collision uses the map's render geometry plus solid static props, plus
the parts of the map's own collision (`CM_PRIMITIVES`: tricoll meshes and brushes) that nothing
rendered covers: `TOOLSNODRAW` backing surfaces (Forwardbase Kodai has a hidden floor 32 units under
its ground and lids over pits, which the Pilot used to fall through into the rooms under the map)
and glass are solid; `TOOLSCLIP` and clip brushes stop only movement, the player clips the Pilot and
the titan clips BT, not shots, the grapple or other traces. Contents bits are read from which
surfaces carry them (0x800000 player, 0x400000 bullets, 0x200000 titans), as bsp_tool has no names
for Titanfall 2's set. `TF_NO_MAP_COLLISION=1` leaves this out; `TF_COLLISION_SWEEP=1` (with it)
logs where the map's collision would catch a fall the render meshes let through.

For automated tests, `--script "0.5-3:fwd,2:dash,2-2.3:left,3-4:turn=-90,4.5:cockpit"` drives the
input and `--shots "2.2:a.png,6:b.png"` saves frames and quits. `TF_WAVE=N` starts the first
game at wave N.

### Milestone 1 (done): BT in a Titanfall map

```
cargo run --release -p tf-viewer -- [--game DIR] [--map mp_forwardbase_kodai] [--seq bt_casual_idle,...]
```

`--screenshot out.png --after 4` saves a frame and quits; `--cam "x y z" --look "x y z"` start in
the free camera at a position in game units; `--no-props` skips static props.

### Layout

- `crates/tf-assets`: format readers
  - `vpk`: Respawn VPKs (LZHAM-compressed parts; LZHAM alpha decompressor vendored, MIT)
  - `rpak` + `rpak_decompress`: RPAK v7 and the RTech decompressor
  - `texture`, `material`: `txtr` v8 (streamed mips from starpaks), `matl` v12
  - `bsp`: rBSP v37 world meshes, static props, entities
  - `mdl`: studio model v53 (embedded VTX/VVD, skins, body groups, RLE animations)
  - `pcf`: particle system definitions (DMX binary 5)
  - `examples/`: CLI inspectors (`vpkls`, `rpakls`, `matls`, `bspinfo`, `mdlinfo`, `seqls`, `animdbg`, ...)
- `crates/tf-sim`: engine-independent simulation (collision BVH, Titan and Pilot movement steps; unit tests)
- `crates/tf-viewer`: the Bevy app (rendering, input, cameras, HUD)


### Known gaps

- Fog/godray cards and scrolling/fading effect surfaces have a shader path (`uber.rs`: the
  shader set name's features `Unlit/Add/Trans/Vcolt/Vcola/Adf/Aef/Uv1at` and the material's CPU
  constants: UV transforms, tints, distance/edge fades). It costs about 5% of the frame rate
  (98.6 against 93.5 fps on Forwardbase Kodai: fragments the distance or edge fade hides are
  dropped before any texture read, and unlit surfaces skip lighting); `TF_NO_UBER=1` turns it
  off, which also leaves out the atmosphere cards and godrays and lights BT's cockpit console
  (an unlit material in the game) like the world. Opacity masks are merged into the colour's
  alpha on the CPU at up to 256 texels, BC7 colour maps included (Kodai's dust clouds are a
  white BC7 colour with a BC4 mask). `TF_NO_VSYNC=1` runs uncapped for frame-rate measurements
  (`RUST_LOG=tf_viewer=info` logs `perf:` lines). `matcpu` and `shdsls` examples dump the
  constants and shader set names.
  With `TF_OLD_SHIELDS=1` effect models use it when their additive material scrolls or
  edge-fades (otherwise they draw with the game's effect shader, see Shield looks): the Thermal
  Shield's flame dome (`fx\flame_shield_edge`) scrolls its caustic texture through UV1, takes
  the high-contrast caustic opacity map (slot 13) as alpha, and is warped by a cloud offset map
  (slot 18) scrolled through UV2. The slot-to-transform reading and the warp amplitude (0.05) are
  guesses. The tint is the system colour, linearised, times its HDR scale (5).
- Reflections use the cubemap nearest the camera by default. `TF_CUBEMAP_PROBES=1` places one
  box reflection probe per baked cubemap (each surface reflects its nearest capture), also off
  by default for the same frame-rate cost. Spec maps
  are reduced to their luminance (no coloured metal tint). Decals keep a plain rough
  dielectric. The cubemap and probe brightness scales were set by eye on Forwardbase Kodai.
- Opacity masks (`_opa` textures of `Opam` shaders: glass panes, the Archer's sight lens) are
  baked into the base colour's alpha and blended; other shader features stay on the plain path.
- RPAK patch chains (`name(01).rpak`...) are not applied as deltas; base paks are used, plus each
  `common(NN).rpak` and `common_mp(NN).rpak` patch's own pages (newest first) for assets the base pak lacks (e.g. the R-101
  SFP's materials). Set `TF_NO_PATCH_PAKS=1` to skip that (~0.75 s at startup).
- Collision uses render meshes plus the map's collision where nothing rendered covers it (see Titan); render meshes the game doesn't collide with (wires, some decals) still block, wall-running and mantling ignore clip surfaces, and the 57 props on Kodai that are marked solid but have no collision primitive still block. Props collide with their render triangles, not their collision models, so the Pilot can catch on a prop's detail the game would smooth over (about 1 in 5 of the dead stops a scan of Angel City found are on such geometry).
- Screen-space particle effects are placed by a guessed mapping and drawn under the viewmodel;
  particle light sources are off by default (see
  Effects). Particle refraction approximates Source's Refract shader (see Effects); shield and
  wall models use the game's effect shader without its height fog. Bullet holes are flat quads, not projected decals, so they don't
  wrap around edges. Point-blank
  hits on BT's hull play the game's systems at the game's sizes (Tone's 40mm bursts are
  100-unit fireballs 50 units from the eye), so a Titan shelling the cockpit still fills the
  view with fire. The cockpit's main screen is left open instead of showing a camera feed.
  The parts of the console that the screen covers in the game (a frame of thin cables lying
  on the screen surface) are left out too: whole connected pieces of the mesh whose vertices
  are mostly behind the screen, or within 3% in front of it, seen from `jx_c_camera` in
  `atpov_cockpit_hatch_close_idle` (`mdl::screen_cull`; the 3% is tuned by eye).
- The Pilot's third-person body (`pilotbody.rs`, Cooper's `sp_medium_reaper_m.mdl`) is shown
  only in the free camera and during executions. Execution cameras ride
  the attacker's animated `jx_c_camera` bone with its own orientation (forward = bone Z, up =
  bone Y; `TF_EXEC_LOOKAT=1` looks at the victim from it instead). The HUD (except subtitles)
  is hidden while an execution plays.
- Enemy Titans use their main weapon, their MP ordnance and (except Northstar) their
  defensive, but no cores. Defensives play the game's effects, BT's included
  (`wpn_vortex_chargingCP_titan`, the system the Vortex script plays, `P_wpn_HeatShield`, `P_titan_gun_shield_3P` with the hex shield
  model; in the cockpit BT's own use the `_FP` systems the game plays on the viewmodel, placed
  at the shield point rather than on a viewmodel attachment (the Vortex at the blocker's
  `vortex_center` pose, baked from its draw sequence), so Scorch's fan is a
  screen-filling tint there); a round the vortex catches hangs in it as `wpn_vortex_projectile_rifle`. Particle
  Walls (BT's too) are the game's `P_xo_shield_wall`. `TF_OLD_FX=1` keeps the glow and pane
  stand-ins.
  Grunts have no cover use, grenades or squad behaviour (they hold their spot and shoot, and
  only move to close in or flank), and Titans can't step on them. On campaign maps enemy Titans
  close in along the game's Titan navmesh (`maps/navmesh/<map>_large.nm`, `tf_assets::navmesh`,
  `nav.rs`: A* over the polys, waypoints at the shared edges, straightened by line-of-sight
  checks; replanned every 2 s or when the goal moves 200 units) and grunts along the `_small`
  one; circling and backing off steer directly. The MP maps ship no client navmesh, so there
  a grid is probed from the collision mesh at load (`nav::GridNav`: 128-unit cells for Titans
  within 6000 of the arena, 64 for grunts within 4000; every surface a downward ray finds in
  a cell is a node if it is walkable (slope, standing room, nothing within the hull at chest
  and step height), neighbours within step-up / drop range are linked when three rays abreast
  between the cell centres are clear, islands under 12 cells are ignored; ~0.5 s and 1.5 s on
  Kodai, `TF_NAV_DUMP=<dir>` writes them as PGMs). Enemy spawn spots with no walking route to
  the player are dropped (a roof the ring probes landed on). A Titan that walks into
  something replans from where it is and follows waypoints one by one for 3 s; waypoints on
  another level are never cut to, however clear the line of sight. Enemies keep apart only
  from their own kind on their own level (grunts below a deck used to hold a Titan on it). Like the player, enemies move in fixed 120 Hz substeps, so a
  long frame (the first shader compiles when effects start playing) can't change their path.
- The engine-side parts of NPC aiming (how proficiency bias shapes the spread, turn rates) are
  approximations; the script-side numbers are the game's.
- Embark, disembark and eject play the game's first-person sequences on the Pilot's arms
  (`fparms.rs`: `ptpov_mount_buddy_stand_front/behind` and `ptpov_dismount_buddy_stand` on BT's
  `HIJACK` attachment, `atpov_cockpit_eject` from the eye; the view rides the arms' camera bone
  and blends in over 0.35 s), with BT's matching sequences. Out of combat (no live enemy
  within 1500 of BT, the XO-16's `npc_min_engage_titan`, standing in for his NPC state not
  being combat/alert) the kneeling set
  plays instead (`at_mount_kneel_*` with `ptpov_mount_buddy_kneel_*`: BT drops to a knee and
  lifts you in), like `ShouldDoRegularEmbark` does for BT in the campaign. The nuclear eject (an MP
  kit) isn't used.
