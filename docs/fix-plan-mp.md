# Fix plan: two-player play test (2026-10-18)

Everything from the first PvP session with two players. Same rules as `fix-plan.md`: the game's
data and the decompiled client/server first, no tuning by eye, one item at a time, and the user
tests each fix before the next one starts.

Groups: **N** network/multiplayer, **T** Titan, **P** Pilot, **W** weapon, **U** UI/menus.

## Working order: easiest to hardest

| # | Item | Group | Likely cause / where to look |
|---|------|-------|------------------------------|
| 1 | Remove Titan third person | U | The View bind (C) toggles `CameraMode::Third` in player.rs. |
| 2 | Eject is X, E, E, E; doomed it is E, E, E | T | pilotctl.rs eject counts presses of Use; the game arms with Use and fires with the eject button. |
| 3 | Cloak lasts too long | P | Duration not read from the ability script. |
| 4 | Phase Shift should last longer | P | Same: duration from the script. |
| 5 | Titan dash lasts too long / floaty | T | Dash impulse and friction vs `titan_*` player settings. |
| 6 | Ronin's Leadwall fires a bit too slowly | W | fire_rate from mp_titanweapon_leadwall. |
| 7 | Wingman Elite is a primary, not a sidearm | W | pilotweapon ARSENAL slot. |
| 8 | Thunderbolt projectile is too fast | W | projectile_launch_speed of mp_weapon_arc_launcher. |
| 9 | Archer fires only when locked on (MP) | W | mp_weapon_rocket_launcher lock rules. |
| 10 | Legion spins up too slowly, minigun too strong; Close Range mode uses 2 rounds a shot | W | Predator Cannon spin-up, damage and ammo_per_shot per mode. |
| 11 | Ion does too much damage | W | Splitter Rifle damage values, MP vs SP keys. |
| 12 | Grenades do too little damage | P | Frag/arc/fire star damage keys. |
| 13 | Stim: proper speed boost, sprint instantly while active | P | Stim speed scale; sprint wind-up skipped while stimmed. |
| 14 | View bob too strong, especially on jumps | P | Viewmodel/camera bob on jump and land. |
| 15 | Light, medium, heavy Titans: health and dash count | T | Per-class titan settings (health, dash charges). |
| 16 | Titan drops too fast after Northstar's hover | T | Gravity/velocity after Hover ends. |
| 17 | Laser Core: turn rate limit | T | Core's turn-rate scale. |
| 18 | Northstar no-scope spread and crosshair | W | Hip spread of the Plasma Railgun and the crosshair's spread. |
| 19 | Scorch damages himself with his own fire | T | Self damage of thermite/firewall/flame core. |
| 20 | EPG: splash and self damage | W | Explosion keys and self damage. |
| 21 | Sidewinder: visible rockets, splash and self damage | W | Projectile model/fx and explosion keys. |
| 22 | Softball flies in an arc; MGL and Softball show the trajectory | W | Projectile gravity; the arc preview. |
| 23 | Missing ADS reticles (R-101, EPG, others); Thunderbolt ADS while moving | W | Optic/reticle per weapon; ADS bob. |
| 24 | P2016 shows its ammo count on the gun | W | The gun's ammo counter screen. |
| 25 | Gun recoil feels wrong (check the decomp) | W | Recoil/kick model vs the decompiled code. |
| 26 | Grapple pulls too hard (unsure) | P | Grapple pull vs the decompiled values. |
| 27 | Gravity Star doesn't pull players | P | Pull applies only to enemies. |
| 28 | Auto-Titan follow / guard mode switch | T | Missing command. |
| 29 | Cores don't show draining | U | Core meter while the core is active. |
| 30 | Escape makes the Titan HUD move around | U | HUD sway keeps running when paused. |
| 31 | Pausing in multiplayer freezes your animations for others and makes you invincible | N | Pause stops the game clock; online it must not. |
| 32 | A dead player can still move and shoot until the respawn | N | Death doesn't stop input/the snapshot until respawn. |
| 33 | Titan nametags show through walls | N/U | No visibility test for the name label. |
| 34 | Cloak doesn't hide the player from others | N | Cloak state not sent. |
| 35 | Arc Wave, shields and other ability effects aren't synced | N | Ability fx not sent. |
| 36 | Shots go through shields: Tone's wall, the A-Wall, some through Vortex | N/W | Walls/shields not checked by all shot types or by remote copies. |
| 37 | Anti-Titan weapons can't kill a Titan | N/W | Doomed Titan damage or final kill not applied remotely. |
| 38 | Titanfall damages enemies, enemy Titans and the caller | T | Landing damage. |
| 39 | Titans run over Pilots | T | Crush damage on contact. |
| 40 | Ronin: sword for melee, Sword Block put away sooner, Sword Core works | T | Ronin melee and block anims, core. |
| 41 | Rodeo: the victim still sees the rider in front and can shoot them | N | Rider position on the victim's screen. |
| 42 | Executions: broken anims for both, can disembark during one, victim should die not eject | N/T | Synced execution over the network. |
| 43 | Lag starts midway through a session | N | Something grows each frame (entities, ghost rounds, queues). |
| 44 | Pilot abilities/weapons missing (e.g. Smart Pistol) | P/W | Roster vs the game. |
| 45 | Titan and Pilot customization screen (consistent upgrades) | U | Loadout menu; kits pick upgrades inconsistently. |

## Status (2026-10-19)

All 45 items were worked through; the user's play test is still to come.

- 25 (recoil) and 26 (grapple pull) were checked against the decompiled client and left as they
  are: they already match the game.
- 44: every Pilot weapon (Smart Pistol included), all seven tacticals and all six ordnance are in.
  Boosts (the earn-meter rewards) are not implemented yet.
- 45: the LOADOUT page picks two Pilot kits, the Titan's own kit, a general Titan kit and a
  Titanfall kit from the game's passive tables (`titan_passives`, `pilot_passives`, plus the six
  later Titan kits), saved in settings.cfg. Each kit is applied the way `_passives.gnut` does it:
  the same-named weapon mod or `ClassMods` block, or the script logic behind it.
  - Not done yet: Threat Optics (the engine's threat-scope highlight), Counter Ready (it adds a
    charge to the core-meter Electric Smoke, which comes with Boosts). Assault Chip only raises
    the auto-Titan's proficiency (AVERAGE to GOOD); the auto-Titan uses no abilities yet, with
    or without it.
  - Dome Shield: the dome, its pulses and its fade work; its colour for an enemy viewer is the
    system's own (the engine's team colour constants are not in the scripts).

## Audit (2026-10-19)

The user's second play test found items 2 and 28 not working, so every item was checked again,
with two-player headless runs where the item is about multiplayer.

- 2, 28: the code assumed the game's default binds, but older settings.cfg files had X on Use
  and the Titan call on `+scriptcommand1`. Binds now migrate once (`binds_version 2`): E use,
  X eject, middle mouse Titan utility. V on foot calls the Titan or switches Follow/Guard.
  The HUD key boxes show the bound keys.
- 3: cloak stays 15 s (MP_BASE `fire_duration`), but a Pilot gun's shot now ends it
  (`breaks_cloak`, on by default in the engine's weapon table, off only on the tacticals).
- 36: Tone's Particle Wall used the campaign's health and duration; MP is 2000 for 6 s.
  Other campaign-only rules found the same way: Firewall and T-203 thermite duration scales,
  the T-203 thermite throw (0.25 in MP), Multi-Target Missile speed (2200 in MP).
- 42: every class played Tone's execution, which only the medium chassis has. Executions now
  come from `titan_executions.rpak` per class, and other players' Titans use their class's
  model (they were all Tone's). The local Titan is still Tone's chassis in play; a stand-in of
  the class's model plays the execution.
- Verified in pair runs: 31 (a paused host still takes damage), 34 (a cloaked Pilot is a faint
  outline for the other player), 36 (shots stop at the Particle Wall until it breaks), 37 (an
  anti-Titan weapon dooms and kills), 39 (a Titan kills a Pilot by walking into them), 42 (no
  disembark or eject during an execution; the Pilot dies with the Titan), 43 (61 fps on both
  sides through three minutes of fire), the nametag of a player who left.
- Found, not on the list: players and Titans don't block each other's movement (no
  character-to-character collision); the player's own Titan, seen from outside, is always
  Tone's model and Tone's cockpit is used for every class.
