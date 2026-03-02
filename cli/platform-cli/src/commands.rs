/// Expand shorthand commands to full session create parameters.
///
/// `platform-cli dev "fix bug"` → `"/dev fix bug"` with persistent mode.
/// `platform-cli plan "add caching"` → `"/plan add caching"` with oneshot mode.
#[allow(dead_code)] // persistent used in tests, will be used for session mode hints
pub struct ExpandedCommand {
    pub prompt: String,
    pub persistent: bool,
}

/// Known shorthand commands and their session modes.
const SHORTHANDS: &[(&str, bool)] = &[
    ("dev", true),           // persistent
    ("plan", false),         // oneshot
    ("review", false),       // oneshot
    ("plan-review", false),  // oneshot
    ("finalize", false),     // oneshot
];

/// Expand a shorthand command name + args into a full prompt.
///
/// Returns `None` if the name is not a recognized shorthand.
pub fn expand_shorthand(name: &str, args: &str) -> Option<ExpandedCommand> {
    SHORTHANDS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, persistent)| {
            let prompt = if args.is_empty() {
                format!("/{name}")
            } else {
                format!("/{name} {args}")
            };
            ExpandedCommand {
                prompt,
                persistent: *persistent,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorthand_dev_expands() {
        let cmd = expand_shorthand("dev", "fix bug").unwrap();
        assert_eq!(cmd.prompt, "/dev fix bug");
        assert!(cmd.persistent);
    }

    #[test]
    fn shorthand_plan_expands() {
        let cmd = expand_shorthand("plan", "add caching").unwrap();
        assert_eq!(cmd.prompt, "/plan add caching");
        assert!(!cmd.persistent);
    }

    #[test]
    fn shorthand_review_expands() {
        let cmd = expand_shorthand("review", "").unwrap();
        assert_eq!(cmd.prompt, "/review");
        assert!(!cmd.persistent);
    }

    #[test]
    fn shorthand_unknown_returns_none() {
        assert!(expand_shorthand("unknown", "args").is_none());
    }

    #[test]
    fn shorthand_finalize_expands() {
        let cmd = expand_shorthand("finalize", "").unwrap();
        assert_eq!(cmd.prompt, "/finalize");
        assert!(!cmd.persistent);
    }
}
