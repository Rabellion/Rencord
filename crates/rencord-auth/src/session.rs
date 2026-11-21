//! What the window is allowed to do with a saved session.
//!
//! The machine does not store the token. Callers keep that in the keychain
//! and only tell the machine whether validation succeeded.

/// Why the window is back on the sign-in screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    FirstRun,
    InvalidSession,
    LoggedOut,
    StorageUnavailable,
}

/// Phases of a saved session. `Checking` is the startup or switch validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    SignedOut,
    Checking,
    Active { user_id: String },
    NeedsLogin { reason: EndReason },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMachine {
    phase: SessionPhase,
}

impl Default for SessionMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMachine {
    pub fn new() -> Self {
        Self {
            phase: SessionPhase::SignedOut,
        }
    }

    pub fn phase(&self) -> &SessionPhase {
        &self.phase
    }

    pub fn is_active(&self) -> bool {
        matches!(self.phase, SessionPhase::Active { .. })
    }

    /// No saved account. The welcome screen is the next step.
    pub fn first_run(&mut self) {
        self.phase = SessionPhase::NeedsLogin {
            reason: EndReason::FirstRun,
        };
    }

    /// Drop whatever phase we were in and validate again. Used when switching accounts.
    pub fn restart(&mut self) {
        self.phase = SessionPhase::Checking;
    }

    /// Start `/users/@me` for a token that is already in the keychain.
    pub fn start_check(&mut self) -> Result<(), SessionPhase> {
        match self.phase {
            SessionPhase::SignedOut | SessionPhase::NeedsLogin { .. } => {
                self.phase = SessionPhase::Checking;
                Ok(())
            }
            _ => Err(self.phase.clone()),
        }
    }

    /// Validation succeeded. `user_id` is the snowflake, not a secret.
    pub fn established(&mut self, user_id: impl Into<String>) -> Result<(), SessionPhase> {
        if !matches!(self.phase, SessionPhase::Checking) {
            return Err(self.phase.clone());
        }
        self.phase = SessionPhase::Active {
            user_id: user_id.into(),
        };
        Ok(())
    }

    /// A 401 from any route. The caller deletes the stored token.
    pub fn unauthorized(&mut self) {
        if matches!(
            self.phase,
            SessionPhase::Checking | SessionPhase::Active { .. }
        ) {
            self.phase = SessionPhase::NeedsLogin {
                reason: EndReason::InvalidSession,
            };
        }
    }

    /// Logout finished (or the local half of it did).
    pub fn logged_out(&mut self) {
        self.phase = SessionPhase::NeedsLogin {
            reason: EndReason::LoggedOut,
        };
    }

    pub fn storage_unavailable(&mut self) {
        self.phase = SessionPhase::NeedsLogin {
            reason: EndReason::StorageUnavailable,
        };
    }

    /// Map an HTTP status. `true` means the session was just invalidated.
    pub fn observe_status(&mut self, status: u16) -> bool {
        if status == 401 {
            let was_live = matches!(
                self.phase,
                SessionPhase::Checking | SessionPhase::Active { .. }
            );
            self.unauthorized();
            was_live
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_401_and_logout_return_to_login() {
        let mut session = SessionMachine::new();
        assert_eq!(session.phase(), &SessionPhase::SignedOut);
        assert!(session.established("1").is_err());

        session.first_run();
        assert_eq!(
            session.phase(),
            &SessionPhase::NeedsLogin {
                reason: EndReason::FirstRun
            }
        );

        session.start_check().unwrap();
        assert!(session.observe_status(401));
        assert_eq!(
            session.phase(),
            &SessionPhase::NeedsLogin {
                reason: EndReason::InvalidSession
            }
        );

        session.start_check().unwrap();
        session.established("42").unwrap();
        assert!(session.is_active());
        assert!(session.start_check().is_err());
        assert!(!session.observe_status(200));
        assert!(session.observe_status(401));
        assert_eq!(
            session.phase(),
            &SessionPhase::NeedsLogin {
                reason: EndReason::InvalidSession
            }
        );

        session.start_check().unwrap();
        session.established("42").unwrap();
        session.logged_out();
        assert_eq!(
            session.phase(),
            &SessionPhase::NeedsLogin {
                reason: EndReason::LoggedOut
            }
        );

        session.storage_unavailable();
        assert_eq!(
            session.phase(),
            &SessionPhase::NeedsLogin {
                reason: EndReason::StorageUnavailable
            }
        );
        let rendered = format!("{session:?}");
        assert!(!rendered.contains("token"));
    }
}
