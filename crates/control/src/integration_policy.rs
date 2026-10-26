use mycelium_proto::api::{IntegrationPolicy, PolicyLevel, ToolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Default,
    Type,
    Tool,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    pub ask: bool,
    pub review: bool,
    pub ask_source: Source,
    pub review_source: Source,
}

fn resolve(level: &PolicyLevel, tool: &str, kind: ToolKind, fallback: bool) -> (bool, Source) {
    if let Some(value) = level.tools.get(tool) {
        return (*value, Source::Tool);
    }

    let by_type = match kind {
        ToolKind::Read => level.read,
        ToolKind::Write => level.write,
    };

    if let Some(value) = by_type {
        return (value, Source::Type);
    }

    match level.all {
        Some(value) => (value, Source::Default),
        None => (fallback, Source::Default),
    }
}

pub fn effective(policy: &IntegrationPolicy, tool: &str, kind: ToolKind, ask_fallback: bool) -> Effective {
    let (ask, ask_source) = resolve(&policy.ask, tool, kind, ask_fallback);
    let (review, review_source) = resolve(&policy.review, tool, kind, false);

    Effective { ask, review, ask_source, review_source }
}

pub fn locked() -> Effective {
    Effective { ask: true, review: true, ask_source: Source::Locked, review_source: Source::Locked }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(ask: PolicyLevel) -> IntegrationPolicy {
        IntegrationPolicy { ask, review: PolicyLevel::default() }
    }

    #[test]
    fn nothing_set_falls_back_and_is_marked_inherited() {
        let result = effective(&IntegrationPolicy::default(), "list", ToolKind::Read, true);

        assert!(result.ask);
        assert_eq!(result.ask_source, Source::Default);
        assert!(!result.review);
    }

    #[test]
    fn the_integration_default_applies_to_every_tool() {
        let result = effective(&policy(PolicyLevel { all: Some(false), ..Default::default() }), "send", ToolKind::Write, true);

        assert!(!result.ask);
        assert_eq!(result.ask_source, Source::Default);
    }

    #[test]
    fn a_type_override_beats_the_default() {
        let level = PolicyLevel { all: Some(true), read: Some(false), ..Default::default() };

        assert!(!effective(&policy(level.clone()), "list", ToolKind::Read, true).ask);
        assert!(effective(&policy(level), "send", ToolKind::Write, false).ask);
    }

    #[test]
    fn a_tool_override_beats_the_type_override() {
        let mut level = PolicyLevel { read: Some(false), ..Default::default() };
        level.tools.insert("list".to_string(), true);
        let result = effective(&policy(level), "list", ToolKind::Read, false);

        assert!(result.ask);
        assert_eq!(result.ask_source, Source::Tool);
    }

    #[test]
    fn review_is_resolved_separately_from_asking() {
        let result = effective(&IntegrationPolicy { ask: PolicyLevel::default(), review: PolicyLevel { write: Some(true), ..Default::default() } }, "send", ToolKind::Write, false);

        assert!(result.review);
        assert_eq!(result.review_source, Source::Type);
        assert!(!result.ask);
    }
}
