pub const FAIL_CLEAR_NANOSECONDS: u64 = 1_500_000_000;

#[derive(Debug, Clone, PartialEq)]
pub enum PasteHud {
    Working {
        message: String,
        sent: usize,
        total: usize,
    },
    Failed {
        message: String,
    },
}

impl PasteHud {
    pub fn failure() -> Self {
        Self::Failed {
            message: "Paste failed".into(),
        }
    }

    pub fn working(is_local: bool, sent: usize, total: usize) -> Self {
        Self::Working {
            message: if is_local {
                "Writing image…".into()
            } else {
                "Uploading image…".into()
            },
            sent,
            total,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Working { message, .. } | Self::Failed { message } => message,
        }
    }

    pub fn fraction(&self) -> Option<f64> {
        match self {
            Self::Working { sent, total, .. } => {
                if *total == 0 {
                    Some(0.0)
                } else {
                    Some(((*sent as f64) / (*total as f64)).min(1.0))
                }
            }
            Self::Failed { .. } => None,
        }
    }

    pub fn percent(&self) -> i32 {
        let fraction = self.fraction().unwrap_or(0.0);
        (fraction * 100.0).round() as i32
    }

    pub fn shows_spinner(&self) -> bool {
        matches!(self, Self::Working { .. })
    }

    pub fn accessibility_text(&self) -> String {
        match self {
            Self::Working { message, .. } => format!("{message} {} percent", self.percent()),
            Self::Failed { message } => message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_copy_depends_on_place() {
        assert_eq!(
            PasteHud::working(true, 0, 10),
            PasteHud::Working {
                message: "Writing image…".into(),
                sent: 0,
                total: 10,
            }
        );
        assert_eq!(
            PasteHud::working(false, 4, 10),
            PasteHud::Working {
                message: "Uploading image…".into(),
                sent: 4,
                total: 10,
            }
        );
    }

    #[test]
    fn failure_copy() {
        assert_eq!(
            PasteHud::failure(),
            PasteHud::Failed {
                message: "Paste failed".into()
            }
        );
        assert!(PasteHud::failure().fraction().is_none());
    }

    #[test]
    fn spinner_only_while_working() {
        assert!(PasteHud::working(true, 0, 1).shows_spinner());
        assert!(PasteHud::working(false, 1, 2).shows_spinner());
        assert!(!PasteHud::failure().shows_spinner());
    }

    #[test]
    fn fraction_and_percent() {
        assert_eq!(PasteHud::working(false, 1, 4).fraction(), Some(0.25));
        assert_eq!(PasteHud::working(false, 1, 3).percent(), 33);
        assert_eq!(PasteHud::working(false, 10, 10).percent(), 100);
        assert_eq!(PasteHud::working(false, 0, 0).fraction(), Some(0.0));
    }

    #[test]
    fn accessibility_includes_percent() {
        assert_eq!(
            PasteHud::working(false, 1, 2).accessibility_text(),
            "Uploading image… 50 percent"
        );
        assert_eq!(PasteHud::failure().accessibility_text(), "Paste failed");
    }
}
