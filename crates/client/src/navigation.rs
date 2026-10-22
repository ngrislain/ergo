//! Shared navigation controls and speed policy, independent of the UI and GPU.
pub const MAX_FRAME_DT: f64 = 0.05;
pub const TURN_RATE: f64 = 1.8;
pub const DEFAULT_EYE_HEIGHT: f64 = 4.0;
pub const MIN_EYE_HEIGHT: f64 = 1.0;
pub const MAX_EYE_HEIGHT: f64 = 32.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavigationMode {
    #[default]
    Orbit,
    Fly,
    Walk,
}

impl NavigationMode {
    pub const ALL: [Self; 3] = [Self::Orbit, Self::Fly, Self::Walk];

    pub fn name(self) -> &'static str {
        match self {
            Self::Orbit => "orbit",
            Self::Fly => "fly",
            Self::Walk => "walk",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Orbit => "Orbit",
            Self::Fly => "Fly",
            Self::Walk => "Walk",
        }
    }

    pub fn instructions(self) -> &'static str {
        match self {
            Self::Orbit => "Drag to orbit / Shift + drag to pan / Scroll to zoom",
            Self::Fly => "Arrows steer / A accelerate · Z slow to stop / Drag to look",
            Self::Walk => "↑↓ walk / ←→ turn / A faster · Z slower / Space jump",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    Up,
    Left,
    Down,
    Right,
    Faster,
    Slower,
    Jump,
}

impl Control {
    // Also the directional-pad order; CSS places the first four as arrows.
    pub const ALL: [Self; 7] = [
        Self::Up,
        Self::Left,
        Self::Down,
        Self::Right,
        Self::Faster,
        Self::Slower,
        Self::Jump,
    ];

    /// Keyboard key, pad symbol, and default (walking) accessible label.
    fn binding(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Up => ("ArrowUp", "↑", "Move forward"),
            Self::Left => ("ArrowLeft", "←", "Turn left"),
            Self::Down => ("ArrowDown", "↓", "Move backward"),
            Self::Right => ("ArrowRight", "→", "Turn right"),
            Self::Faster => ("a", "A", "Faster (A)"),
            Self::Slower => ("z", "Z", "Slower (Z)"),
            Self::Jump => (" ", "⤒", "Jump (Space)"),
        }
    }

    pub fn key(self) -> &'static str {
        self.binding().0
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|control| control.key().eq_ignore_ascii_case(key))
    }

    pub fn symbol(self) -> &'static str {
        self.binding().1
    }

    pub fn label(self, mode: NavigationMode) -> &'static str {
        match (self, mode) {
            (Self::Up, NavigationMode::Fly) => "Look up",
            (Self::Down, NavigationMode::Fly) => "Look down",
            _ => self.binding().2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NavigationInput {
    /// Up/down: pitch in Fly, forward/backward in Walk.
    pub vertical: f64,
    pub turn: f64,
    pub throttle: f64,
    pub jump: bool,
}

impl NavigationInput {
    pub fn from_controls(controls: impl IntoIterator<Item = Control>) -> Self {
        let mut input = Self::default();
        for control in controls {
            match control {
                Control::Up => input.vertical += 1.0,
                Control::Down => input.vertical -= 1.0,
                Control::Left => input.turn += 1.0,
                Control::Right => input.turn -= 1.0,
                Control::Faster => input.throttle += 1.0,
                Control::Slower => input.throttle -= 1.0,
                Control::Jump => input.jump = true,
            }
        }
        input
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationStatus {
    pub mode: NavigationMode,
    pub speed: f64,
    pub eye_height: f64,
}

impl Default for NavigationStatus {
    fn default() -> Self {
        Navigation::default().status()
    }
}

struct Speed {
    value: f64,
    acceleration: f64,
    max: f64,
}

impl Speed {
    fn set(&mut self, value: f64) {
        self.value = value.clamp(0.0, self.max);
    }
}

pub struct Navigation {
    mode: NavigationMode,
    fly: Speed,
    walk: Speed,
    eye_height: f64,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            mode: NavigationMode::Orbit,
            eye_height: DEFAULT_EYE_HEIGHT,
            fly: Speed {
                value: 0.0,
                acceleration: 64.0,
                max: 2048.0,
            },
            walk: Speed {
                value: 18.0,
                acceleration: 18.0,
                max: 128.0,
            },
        }
    }
}

impl Navigation {
    pub fn mode(&self) -> NavigationMode {
        self.mode
    }

    pub fn eye_height(&self) -> f64 {
        self.eye_height
    }

    /// Return the camera-height adjustment after applying the shared limits.
    pub fn set_eye_height(&mut self, height: f64) -> f64 {
        let previous = self.eye_height;
        self.eye_height = height.clamp(MIN_EYE_HEIGHT, MAX_EYE_HEIGHT);
        self.eye_height - previous
    }

    pub fn set_mode(&mut self, mode: NavigationMode) -> bool {
        if self.mode == mode {
            return false;
        }
        if mode == NavigationMode::Fly {
            self.fly.value = 0.0;
        }
        self.mode = mode;
        true
    }

    pub fn status(&self) -> NavigationStatus {
        NavigationStatus {
            mode: self.mode,
            eye_height: self.eye_height,
            speed: match self.mode {
                NavigationMode::Orbit => 0.0,
                NavigationMode::Fly => self.fly.value,
                NavigationMode::Walk => self.walk.value,
            },
        }
    }

    fn active_speed(&mut self) -> Option<&mut Speed> {
        match self.mode {
            NavigationMode::Orbit => None,
            NavigationMode::Fly => Some(&mut self.fly),
            NavigationMode::Walk => Some(&mut self.walk),
        }
    }

    pub fn accelerate(&mut self, throttle: f64, dt: f64) {
        if let Some(speed) = self.active_speed() {
            speed.set(speed.value + throttle * speed.acceleration * dt);
        }
    }

    pub fn scroll(&mut self, amount: f64) {
        if let Some(speed) = self.active_speed() {
            // The offset lets wheel and +/- controls accelerate from zero.
            speed.set((speed.value + 1.0) * (-amount * 0.001).exp() - 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposing_controls_cancel_independently_of_input_order() {
        let expected = NavigationInput {
            jump: true,
            ..Default::default()
        };
        assert_eq!(NavigationInput::from_controls(Control::ALL), expected);
        assert_eq!(
            NavigationInput::from_controls(Control::ALL.into_iter().rev()),
            expected
        );
        assert_eq!(Control::from_key("A"), Some(Control::Faster));
        assert_eq!(Control::from_key("ARROWLEFT"), Some(Control::Left));
        assert_eq!(Control::from_key("w"), None);
    }

    #[test]
    fn free_mode_speed_policy_is_bounded_and_wheel_can_restart_it() {
        let mut navigation = Navigation::default();
        for (mode, initial, after_second, max) in [
            (NavigationMode::Fly, 0.0, 64.0, 2048.0),
            (NavigationMode::Walk, 18.0, 36.0, 128.0),
        ] {
            navigation.set_mode(mode);
            assert_eq!(navigation.status().speed, initial);
            navigation.accelerate(1.0, 1.0);
            assert_eq!(navigation.status().speed, after_second);
            navigation.accelerate(1.0, 100.0);
            assert_eq!(navigation.status().speed, max);
            navigation.accelerate(-1.0, 100.0);
            assert_eq!(navigation.status().speed, 0.0);
            navigation.scroll(-180.0);
            assert!(navigation.status().speed > 0.0);
            navigation.scroll(180.0);
            assert!(navigation.status().speed < 1e-12);
        }
    }

    #[test]
    fn mode_transitions_preserve_walking_speed_and_reset_flight_only_on_entry() {
        let mut navigation = Navigation::default();
        navigation.set_mode(NavigationMode::Walk);
        navigation.accelerate(1.0, 1.0);
        navigation.set_mode(NavigationMode::Fly);
        navigation.accelerate(1.0, 1.0);
        assert!(!navigation.set_mode(NavigationMode::Fly));
        assert_eq!(navigation.status().speed, 64.0);
        navigation.set_mode(NavigationMode::Walk);
        assert_eq!(navigation.status().speed, 36.0);
        navigation.set_mode(NavigationMode::Fly);
        assert_eq!(navigation.status().speed, 0.0);
        navigation.set_mode(NavigationMode::Orbit);
        navigation.accelerate(1.0, 1.0);
        navigation.scroll(-180.0);
        assert_eq!(navigation.status(), NavigationStatus::default());
    }

    #[test]
    fn eye_height_is_bounded_persistent_and_reported_from_the_same_state() {
        let mut navigation = Navigation::default();
        assert_eq!(navigation.set_eye_height(6.0), 2.0);
        navigation.set_mode(NavigationMode::Fly);
        navigation.set_mode(NavigationMode::Walk);
        assert_eq!(navigation.status().eye_height, 6.0);
        assert_eq!(navigation.set_eye_height(0.0), -5.0);
        assert_eq!(navigation.eye_height(), 1.0);
        assert_eq!(navigation.set_eye_height(100.0), 31.0);
        assert_eq!(navigation.status().eye_height, 32.0);
    }
}
