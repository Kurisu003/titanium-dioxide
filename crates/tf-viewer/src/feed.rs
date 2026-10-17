//! The kill feed (the game's obituary crawl, top right: killer, the weapon's icon, victim,
//! each row living OBITUARY_DURATION) and the Frontier Defense cash: a "$ +N" popup rising
//! from the kill and the "$ N" counter bottom left.

use crate::game::Game;
use crate::player::MainCamera;
use crate::ui::{UiAssets, VhText};
use bevy::prelude::*;

/// cl_hud.gnut OBITUARY_DURATION.
const ROW_SECS: f32 = 5.0;
const POPUP_SECS: f32 = 1.6;
const MAX_ROWS: usize = 6;

pub struct Kill {
    pub victim: String,
    pub weapon: String,
    pub at: f32,
}

pub struct Popup {
    /// Game units.
    pub pos: Vec3,
    pub worth: u32,
    pub at: f32,
}

#[derive(Resource, Default)]
pub struct Feed {
    pub kills: Vec<Kill>,
    pub popups: Vec<Popup>,
}

#[derive(Component)]
pub struct FeedRow(usize);
#[derive(Component)]
pub struct FeedRowKiller;
#[derive(Component)]
pub struct FeedRowIcon;
#[derive(Component)]
pub struct FeedRowVictim;
#[derive(Component)]
pub struct PopupNode(usize);
#[derive(Component)]
pub struct PopupText;
#[derive(Component)]
pub struct CashText;
#[derive(Component)]
pub struct CashRoot;

const FRIENDLY: Color = Color::srgb(0.45, 0.75, 1.0);
const ENEMY: Color = Color::srgb(1.0, 0.5, 0.2);
const CASH: Color = Color::srgb(1.0, 0.72, 0.1);

fn vh(v: f32) -> Val {
    Val::Vh(v)
}

pub fn spawn(commands: &mut Commands, ui: &UiAssets, root: Entity) {
    // Kill feed rows, right-aligned under the top-right corner.
    let feed = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            right: vh(6.0),
            top: vh(12.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: vh(0.4),
            ..default()
        })
        .id();
    commands.entity(root).add_child(feed);
    for i in 0..MAX_ROWS {
        let row = commands.spawn((Node { display: Display::None, align_items: AlignItems::Center, column_gap: vh(0.6), ..default() }, FeedRow(i))).id();
        let killer = commands.spawn((Text::new("You"), TextFont { font: ui.font.clone(), font_size: 16.0, ..default() }, TextColor(FRIENDLY), VhText(1.9), FeedRowKiller)).id();
        let icon_box = commands.spawn(Node { width: vh(5.0), height: vh(2.0), ..default() }).id();
        let icon = commands.spawn((Node::default(), ImageNode { color: Color::NONE, ..default() }, crate::hud::FitIcon, FeedRowIcon)).id();
        commands.entity(icon_box).add_child(icon);
        let victim = commands.spawn((Text::new(""), TextFont { font: ui.font.clone(), font_size: 16.0, ..default() }, TextColor(ENEMY), VhText(1.9), FeedRowVictim)).id();
        commands.entity(row).add_children(&[killer, icon_box, victim]);
        commands.entity(feed).add_child(row);
    }
    // Cash popups (pooled) and the counter.
    for i in 0..8 {
        let p = commands
            .spawn((Node { position_type: PositionType::Absolute, display: Display::None, align_items: AlignItems::Center, column_gap: vh(0.5), ..default() }, PopupNode(i)))
            .id();
        let d = dollar(commands, ui, 2.6);
        let t = commands.spawn((Text::new(""), TextFont { font: ui.bold_font.clone(), font_size: 16.0, ..default() }, TextColor(Color::WHITE), VhText(2.4), PopupText)).id();
        commands.entity(p).add_children(&[d, t]);
        commands.entity(root).add_child(p);
    }
    let cash = commands
        // Measured on a 720p Frontier Defense frame: the tag at (380, 578) of 1280x720.
        .spawn((Node { position_type: PositionType::Absolute, left: vh(52.8), top: vh(78.5), align_items: AlignItems::Center, column_gap: vh(0.8), ..default() }, CashRoot))
        .id();
    let d = dollar(commands, ui, 3.6);
    let t = commands.spawn((Text::new("0"), TextFont { font: ui.bold_font.clone(), font_size: 16.0, ..default() }, TextColor(Color::WHITE), VhText(4.0), CashText)).id();
    commands.entity(cash).add_children(&[d, t]);
    commands.entity(root).add_child(cash);
}

/// The game's gold dollar tag.
fn dollar(commands: &mut Commands, ui: &UiAssets, size: f32) -> Entity {
    let b = commands.spawn(Node { width: vh(size * 0.62), height: vh(size), ..default() }).id();
    let mut img = ui.image("rui/gencard_icons/gc_icon_dollarsign_small").unwrap_or_default();
    img.color = CASH;
    let i = commands.spawn((Node::default(), img, crate::hud::FitIcon)).id();
    commands.entity(b).add_child(i);
    b
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update(
    mut feed: ResMut<Feed>,
    game: Res<Game>,
    time: Res<Time>,
    ui: Res<UiAssets>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut rows: Query<(&FeedRow, &mut Node, &Children), (Without<PopupNode>, Without<CashRoot>)>,
    mut victims: Query<(&mut Text, &mut TextColor), (With<FeedRowVictim>, Without<PopupText>, Without<CashText>, Without<FeedRowKiller>)>,
    mut killers: Query<&mut TextColor, (With<FeedRowKiller>, Without<FeedRowVictim>, Without<PopupText>, Without<CashText>)>,
    mut icons: Query<&mut ImageNode, With<FeedRowIcon>>,
    mut popups: Query<(&PopupNode, &mut Node, &Children), (Without<FeedRow>, Without<CashRoot>)>,
    mut popup_texts: Query<(&mut Text, &mut TextColor), (With<PopupText>, Without<FeedRowVictim>, Without<CashText>, Without<FeedRowKiller>)>,
    mut cash: Query<&mut Text, (With<CashText>, Without<PopupText>, Without<FeedRowVictim>)>,
    mut cash_root: Query<&mut Node, (With<CashRoot>, Without<FeedRow>, Without<PopupNode>)>,
) {
    let now = time.elapsed_secs();
    feed.kills.retain(|k| now - k.at < ROW_SECS);
    feed.popups.retain(|p| now - p.at < POPUP_SECS);
    let n = feed.kills.len();
    let first = n.saturating_sub(MAX_ROWS);
    for (row, mut node, children) in &mut rows {
        // Newest at the top.
        let k = feed.kills.get(n.checked_sub(1 + row.0).map(|i| i.max(first)).unwrap_or(usize::MAX));
        let Some(k) = k.filter(|_| row.0 < n) else {
            crate::hud::set(&mut node, |n| &mut n.display, Display::None);
            continue;
        };
        crate::hud::set(&mut node, |n| &mut n.display, Display::Flex);
        let alpha = ((ROW_SECS - (now - k.at)) / 0.6).clamp(0.0, 1.0);
        for &c in children {
            if let Ok((mut t, mut col)) = victims.get_mut(c) {
                if t.0 != k.victim {
                    t.0 = k.victim.clone();
                }
                col.0 = ENEMY.with_alpha(alpha);
            } else if let Ok(mut col) = killers.get_mut(c) {
                col.0 = FRIENDLY.with_alpha(alpha);
            } else {
                // The icon box holds the icon.
                for mut img in &mut icons {
                    let _ = &mut img;
                }
            }
        }
        for mut img in &mut icons {
            if let Some(n) = ui.weapon_icons.get(&k.weapon).and_then(|p| ui.image(p)) {
                if img.image != n.image || img.rect != n.rect {
                    img.image = n.image;
                    img.rect = n.rect;
                }
                img.color = Color::WHITE.with_alpha(alpha);
            }
        }
    }
    let cam = cameras.single().ok();
    for (p, mut node, children) in &mut popups {
        let Some(pop) = feed.popups.get(p.0) else {
            crate::hud::set(&mut node, |n| &mut n.display, Display::None);
            continue;
        };
        let age = now - pop.at;
        let screen = cam.and_then(|(c, tf)| c.world_to_viewport(tf, crate::player::to_bevy(pop.pos)).ok());
        let Some(s) = screen else {
            crate::hud::set(&mut node, |n| &mut n.display, Display::None);
            continue;
        };
        crate::hud::set(&mut node, |n| &mut n.display, Display::Flex);
        // Rises 4 vh over its life and fades in the last third.
        crate::hud::set(&mut node, |n| &mut n.left, Val::Px(s.x));
        crate::hud::set(&mut node, |n| &mut n.top, Val::Px(s.y));
        crate::hud::set(&mut node, |n| &mut n.margin, UiRect::top(vh(-age / POPUP_SECS * 4.0)));
        let alpha = ((POPUP_SECS - age) / (POPUP_SECS / 3.0)).clamp(0.0, 1.0);
        for &c in children {
            if let Ok((mut t, mut col)) = popup_texts.get_mut(c) {
                let want = format!("+{}", pop.worth);
                if t.0 != want {
                    t.0 = want;
                }
                col.0 = Color::WHITE.with_alpha(alpha);
            }
        }
    }
    if let Ok(mut t) = cash.single_mut() {
        let want = game.cash.to_string();
        if t.0 != want {
            t.0 = want;
        }
    }
    if let Ok(mut n) = cash_root.single_mut() {
        crate::hud::set(&mut n, |n| &mut n.display, if game.in_play() { Display::Flex } else { Display::None });
    }
}
