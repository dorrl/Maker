use super::*;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub(super) const TIMER: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Effect {
    Hover,
    Focus,
    Press,
    Selected,
    Arrow,
    Reveal,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Key(Handle, usize, Effect);

impl Key {
    pub const fn control(window: Handle, effect: Effect) -> Self {
        Self(window, 0, effect)
    }

    pub const fn item(window: Handle, index: usize, effect: Effect) -> Self {
        Self(window, index, effect)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Area {
    Control,
    Item,
    Rect(Rect),
}

struct Transition {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    area: Area,
}

impl Transition {
    fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32())
            .min(1.0)
    }

    fn value(&self, now: Instant) -> f32 {
        let remaining = 1.0 - self.progress(now);
        self.to + (self.from - self.to) * remaining * remaining * remaining
    }
}

#[derive(Default)]
struct Motion {
    enabled: Option<bool>,
    transitions: HashMap<Key, Transition>,
    timer: bool,
}

thread_local! {
    static MOTION: RefCell<Motion> = RefCell::new(Motion::default());
}

pub(super) fn enabled() -> bool {
    MOTION.with_borrow_mut(|motion| *motion.enabled.get_or_insert_with(client_animations))
}

pub(super) fn refresh(parent: Handle) {
    let enabled = client_animations();
    let changed = MOTION.with_borrow_mut(|motion| motion.enabled.replace(enabled) != Some(enabled));
    if changed && !enabled {
        let running = MOTION.with_borrow_mut(|motion| {
            motion.timer = false;
            std::mem::take(&mut motion.transitions)
        });
        unsafe {
            KillTimer(parent, TIMER);
        }
        for (key, transition) in running {
            invalidate(key, transition.area);
        }
    }
}

pub(super) fn animate(parent: Handle, key: Key, area: Area, from: bool, to: bool) {
    let duration = match key.2 {
        Effect::Press => {
            if to {
                70
            } else {
                115
            }
        }
        Effect::Arrow | Effect::Reveal => 135,
        _ => 120,
    };
    let now = Instant::now();
    let should_start = enabled()
        && MOTION.with_borrow_mut(|motion| {
            let starting = from;
            let to = f32::from(to);
            let mut from = motion
                .transitions
                .get(&key)
                .map_or(f32::from(from), |active| active.value(now));
            if key.2 == Effect::Press && starting && to == 0.0 {
                from = from.max(0.45);
            }
            if (from - to).abs() < 0.001 {
                motion.transitions.remove(&key);
                return false;
            }
            motion.transitions.insert(
                key,
                Transition {
                    from,
                    to,
                    start: now,
                    duration: Duration::from_millis(duration),
                    area,
                },
            );
            if motion.timer {
                false
            } else {
                motion.timer = true;
                true
            }
        });
    if should_start && unsafe { SetTimer(parent, TIMER, 16, 0) } == 0 {
        MOTION.with_borrow_mut(|motion| {
            motion.timer = false;
            motion.transitions.remove(&key);
        });
    }
    invalidate(key, area);
}

pub(super) fn value(key: Key, steady: bool) -> f32 {
    MOTION.with_borrow(|motion| {
        motion
            .transitions
            .get(&key)
            .map_or(f32::from(steady), |transition| {
                transition.value(Instant::now())
            })
    })
}

pub(super) fn clear(window: Handle) {
    MOTION.with_borrow_mut(|motion| motion.transitions.retain(|key, _| key.0 != window));
}

pub(super) fn active(key: Key) -> bool {
    MOTION.with_borrow(|motion| motion.transitions.contains_key(&key))
}

pub(super) fn tick(parent: Handle) {
    let now = Instant::now();
    let (areas, idle) = MOTION.with_borrow_mut(|motion| {
        let mut areas = Vec::with_capacity(motion.transitions.len());
        motion.transitions.retain(|&key, transition| {
            areas.push((key, transition.area));
            transition.progress(now) < 1.0
        });
        let idle = motion.transitions.is_empty();
        motion.timer = !idle;
        (areas, idle)
    });
    if idle {
        unsafe {
            KillTimer(parent, TIMER);
        }
    }
    for (key, area) in areas {
        invalidate(key, area);
    }
}

fn invalidate(key: Key, area: Area) {
    match area {
        Area::Control => repaint(key.0),
        Area::Rect(rect) => repaint_rect(key.0, rect),
        Area::Item => {
            if let Some(rect) = list_item_rect(key.0, key.1) {
                repaint_rect(key.0, rect);
            }
        }
    }
}

pub(super) fn mix(from: u32, to: u32, amount: f32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |color, shift| {
        let lower = ((from >> shift) & 255) as f32;
        let upper = ((to >> shift) & 255) as f32;
        color | ((lower + (upper - lower) * amount).round() as u32) << shift
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_transition_continues_from_current_tint() {
        let start = Instant::now();
        let duration = Duration::from_millis(120);
        let hover = Transition {
            from: 0.0,
            to: 1.0,
            start,
            duration,
            area: Area::Control,
        };
        let reversed_at = start + duration / 2;
        let tint = hover.value(reversed_at);
        assert!(tint > 0.5 && tint < 1.0);
        let reverse = Transition {
            from: tint,
            to: 0.0,
            start: reversed_at,
            duration,
            area: Area::Control,
        };
        assert_eq!(reverse.value(reversed_at), tint);
        assert!(reverse.value(reversed_at + duration / 2) < tint);
        assert_eq!(reverse.value(reversed_at + duration), 0.0);
        assert_eq!(mix(rgb(20, 34, 58), rgb(48, 62, 66), 0.5), rgb(34, 48, 62));
    }
}
