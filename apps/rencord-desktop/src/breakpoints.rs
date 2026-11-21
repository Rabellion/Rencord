//! Viewport widths shared by the shell.
//!
//! Login stacks vertically below [`LOGIN_STACK`] or whenever the window is
//! taller than it is wide. The channel sidebar and member list use the same
//! widths the shell already collapsed at.

pub const SIDEBAR_HIDE: f32 = 760.0;
pub const MEMBERS_HIDE_ONYX: f32 = 900.0;
pub const MEMBERS_HIDE: f32 = 1100.0;
pub const LOGIN_STACK: f32 = 900.0;

pub fn stacks_login(width: f32, height: f32) -> bool {
    let ratio = if height > 0.0 { width / height } else { 1.0 };
    width < LOGIN_STACK
        || matches!(
            super::layout::classify(ratio),
            super::layout::AspectClass::Portrait
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_and_narrow_windows_stack_the_login_screen() {
        assert!(stacks_login(420.0, 800.0));
        assert!(stacks_login(900.0, 1440.0));
        assert!(!stacks_login(1180.0, 760.0));
        assert_eq!(SIDEBAR_HIDE, 760.0);
        assert_eq!(MEMBERS_HIDE_ONYX, 900.0);
    }
}
