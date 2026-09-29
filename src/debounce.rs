/// Button identification and names
pub const BUTTON_LEFT: usize = 0;
pub const BUTTON_RIGHT: usize = 1;
pub const BUTTON_MIDDLE: usize = 2;
pub const BUTTON_BACK: usize = 3;
pub const BUTTON_FORWARD: usize = 4;
pub const MAX_BUTTONS: usize = 32;

pub fn button_name(button_idx: usize) -> &'static str {
    match button_idx {
        BUTTON_LEFT => "Left",
        BUTTON_RIGHT => "Right",
        BUTTON_MIDDLE => "Middle",
        BUTTON_BACK => "Back",
        BUTTON_FORWARD => "Forward",
        _ => "Other",
    }
}

/// Action to take on a mouse event
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebounceAction {
    Pass,
    Drop { reason: DropReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReason {
    /// Second Down arrived too quickly after previous Down
    DownTooQuickAfterDown(u64),
    /// Down arrived too quickly after Up (release bounce)
    DownTooQuickAfterUp(u64),
    /// Button is already logically held down
    DuplicateDownWhileHeld,
    /// Up corresponds to a previously suppressed Down bounce
    PairedBounceUp,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonState {
    pub last_down_ns: u64,
    pub last_up_ns: u64,
    pub is_down: bool,
    pub suppressed_down: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSelection {
    All,
    Specific(u32),
}

impl ButtonSelection {
    pub fn should_debounce(&self, button_idx: usize) -> bool {
        match self {
            ButtonSelection::All => true,
            ButtonSelection::Specific(mask) => {
                if button_idx < MAX_BUTTONS {
                    (mask & (1 << button_idx)) != 0
                } else {
                    false
                }
            }
        }
    }

    /// Computes the CGEventMask for CGEventTapCreate
    pub fn cg_event_mask(&self) -> u64 {
        const LEFT_MASK: u64 = (1 << 1) | (1 << 2);
        const RIGHT_MASK: u64 = (1 << 3) | (1 << 4);
        const OTHER_MASK: u64 = (1 << 25) | (1 << 26);

        match self {
            ButtonSelection::All => LEFT_MASK | RIGHT_MASK | OTHER_MASK,
            ButtonSelection::Specific(mask) => {
                let mut cg_mask = 0u64;
                if (mask & (1 << 0)) != 0 {
                    cg_mask |= LEFT_MASK;
                }
                if (mask & (1 << 1)) != 0 {
                    cg_mask |= RIGHT_MASK;
                }
                // Buttons 2 and above use kCGEventOtherMouseDown / Up
                if (*mask >> 2) != 0 {
                    cg_mask |= OTHER_MASK;
                }
                cg_mask
            }
        }
    }
}

pub struct Debouncer {
    pub debounce_ns: u64,
    pub selection: ButtonSelection,
    pub buttons: [ButtonState; MAX_BUTTONS],
}

impl Debouncer {
    pub fn new(debounce_ms: u64, selection: ButtonSelection) -> Self {
        Self {
            debounce_ns: debounce_ms.saturating_mul(1_000_000),
            selection,
            buttons: [ButtonState::default(); MAX_BUTTONS],
        }
    }

    pub fn process_down(&mut self, button_idx: usize, timestamp_ns: u64) -> DebounceAction {
        if button_idx >= MAX_BUTTONS || !self.selection.should_debounce(button_idx) {
            return DebounceAction::Pass;
        }

        let state = &mut self.buttons[button_idx];

        // 1. If button is already pressed, this is a bounce or duplicate press
        if state.is_down {
            state.suppressed_down = true;
            return DebounceAction::Drop {
                reason: DropReason::DuplicateDownWhileHeld,
            };
        }

        // 2. Check elapsed time since last Down
        if state.last_down_ns > 0 {
            let elapsed = timestamp_ns.saturating_sub(state.last_down_ns);
            if elapsed < self.debounce_ns {
                state.suppressed_down = true;
                return DebounceAction::Drop {
                    reason: DropReason::DownTooQuickAfterDown(elapsed / 1_000_000),
                };
            }
        }

        // 3. Check elapsed time since last Up (release chatter)
        if state.last_up_ns > 0 {
            let elapsed = timestamp_ns.saturating_sub(state.last_up_ns);
            if elapsed < self.debounce_ns {
                state.suppressed_down = true;
                return DebounceAction::Drop {
                    reason: DropReason::DownTooQuickAfterUp(elapsed / 1_000_000),
                };
            }
        }

        // Legitimate press
        state.is_down = true;
        state.last_down_ns = timestamp_ns;
        state.suppressed_down = false;
        DebounceAction::Pass
    }

    pub fn process_up(&mut self, button_idx: usize, timestamp_ns: u64) -> DebounceAction {
        if button_idx >= MAX_BUTTONS || !self.selection.should_debounce(button_idx) {
            return DebounceAction::Pass;
        }

        let state = &mut self.buttons[button_idx];

        // If the corresponding Down was suppressed, swallow this Up bounce too
        if state.suppressed_down {
            state.suppressed_down = false;
            // Update last_up_ns to extend lockout if switch continues to chatter
            state.last_up_ns = timestamp_ns;
            return DebounceAction::Drop {
                reason: DropReason::PairedBounceUp,
            };
        }

        // If the button wasn't considered down, swallow rogue Up
        if !state.is_down {
            state.last_up_ns = timestamp_ns;
            return DebounceAction::Drop {
                reason: DropReason::PairedBounceUp,
            };
        }

        // Legitimate release
        state.is_down = false;
        state.last_up_ns = timestamp_ns;
        DebounceAction::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000; // 1 ms in nanoseconds

    #[test]
    fn test_clean_single_click() {
        let mut debouncer = Debouncer::new(50, ButtonSelection::All);

        assert_eq!(debouncer.process_down(0, 100 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_up(0, 180 * MS), DebounceAction::Pass);
    }

    #[test]
    fn test_deliberate_double_click() {
        let mut debouncer = Debouncer::new(50, ButtonSelection::All);

        // First click
        assert_eq!(debouncer.process_down(0, 100 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_up(0, 160 * MS), DebounceAction::Pass);

        // Second click after 100ms (above 50ms threshold)
        assert_eq!(debouncer.process_down(0, 260 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_up(0, 320 * MS), DebounceAction::Pass);
    }

    #[test]
    fn test_release_bounce_chatter_suppressed() {
        let mut debouncer = Debouncer::new(50, ButtonSelection::All);

        // Real click
        assert_eq!(debouncer.process_down(0, 100 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_up(0, 180 * MS), DebounceAction::Pass);

        // Bounce click 15ms after release
        assert_eq!(
            debouncer.process_down(0, 195 * MS),
            DebounceAction::Drop {
                reason: DropReason::DownTooQuickAfterUp(15)
            }
        );
        assert_eq!(
            debouncer.process_up(0, 202 * MS),
            DebounceAction::Drop {
                reason: DropReason::PairedBounceUp
            }
        );

        // Next legitimate click after debounce period
        assert_eq!(debouncer.process_down(0, 300 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_up(0, 370 * MS), DebounceAction::Pass);
    }

    #[test]
    fn test_press_bounce_chatter_suppressed() {
        let mut debouncer = Debouncer::new(50, ButtonSelection::All);

        // Real press
        assert_eq!(debouncer.process_down(0, 100 * MS), DebounceAction::Pass);

        // Rapid second Down while already down
        assert_eq!(
            debouncer.process_down(0, 110 * MS),
            DebounceAction::Drop {
                reason: DropReason::DuplicateDownWhileHeld
            }
        );

        // Paired bounce up from duplicate press is swallowed
        assert_eq!(
            debouncer.process_up(0, 115 * MS),
            DebounceAction::Drop {
                reason: DropReason::PairedBounceUp
            }
        );

        // Final real release
        assert_eq!(debouncer.process_up(0, 200 * MS), DebounceAction::Pass);
    }

    #[test]
    fn test_selective_button_debouncing() {
        // Only debounce Left button (bit 0)
        let mut debouncer = Debouncer::new(50, ButtonSelection::Specific(1 << 0));

        // Left button bounce is dropped
        assert_eq!(debouncer.process_down(0, 100 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_up(0, 150 * MS), DebounceAction::Pass);
        assert_eq!(
            debouncer.process_down(0, 160 * MS),
            DebounceAction::Drop {
                reason: DropReason::DownTooQuickAfterUp(10)
            }
        );

        // Right button (button 1) is NOT debounced and always passes through immediately
        assert_eq!(debouncer.process_down(1, 100 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_up(1, 150 * MS), DebounceAction::Pass);
        assert_eq!(debouncer.process_down(1, 160 * MS), DebounceAction::Pass);
    }

    #[test]
    fn test_event_masks() {
        let all = ButtonSelection::All;
        assert_eq!(
            all.cg_event_mask(),
            (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 25) | (1 << 26)
        );

        let left_only = ButtonSelection::Specific(1 << 0);
        assert_eq!(left_only.cg_event_mask(), (1 << 1) | (1 << 2));

        let right_only = ButtonSelection::Specific(1 << 1);
        assert_eq!(right_only.cg_event_mask(), (1 << 3) | (1 << 4));

        let middle_only = ButtonSelection::Specific(1 << 2);
        assert_eq!(middle_only.cg_event_mask(), (1 << 25) | (1 << 26));

        let back_only = ButtonSelection::Specific(1 << BUTTON_BACK);
        assert_eq!(back_only.cg_event_mask(), (1 << 25) | (1 << 26));
    }

    #[test]
    fn test_back_forward_button_debouncing() {
        // Debounce only back button (button 3)
        let mut debouncer = Debouncer::new(50, ButtonSelection::Specific(1 << BUTTON_BACK));

        // Back button chatter is suppressed
        assert_eq!(
            debouncer.process_down(BUTTON_BACK, 100 * MS),
            DebounceAction::Pass
        );
        assert_eq!(
            debouncer.process_up(BUTTON_BACK, 150 * MS),
            DebounceAction::Pass
        );
        assert_eq!(
            debouncer.process_down(BUTTON_BACK, 160 * MS),
            DebounceAction::Drop {
                reason: DropReason::DownTooQuickAfterUp(10)
            }
        );

        // Forward button (button 4) is untouched and passes through immediately
        assert_eq!(
            debouncer.process_down(BUTTON_FORWARD, 100 * MS),
            DebounceAction::Pass
        );
        assert_eq!(
            debouncer.process_up(BUTTON_FORWARD, 150 * MS),
            DebounceAction::Pass
        );
        assert_eq!(
            debouncer.process_down(BUTTON_FORWARD, 160 * MS),
            DebounceAction::Pass
        );
    }
}
