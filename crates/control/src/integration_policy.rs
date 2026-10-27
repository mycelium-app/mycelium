use mycelium_proto::api::{ApprovalPreset, IntegrationPolicy, PolicyLevel, ToolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Preset,
    Type,
    Tool,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Preset => "preset",
            Source::Type => "type",
            Source::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    pub ask: bool,
    pub edit: bool,
    pub ask_source: Source,
    pub edit_source: Source,
}

fn resolve(level: &PolicyLevel, tool: &str, kind: ToolKind, preset: bool) -> (bool, Source) {
    if let Some(value) = level.tools.get(tool) {
        return (*value, Source::Tool);
    }

    let by_type = match kind {
        ToolKind::Read => level.read,
        ToolKind::Write => level.write,
    };

    match by_type {
        Some(value) => (value, Source::Type),
        None => (preset, Source::Preset),
    }
}

pub fn effective(policy: &IntegrationPolicy, tool: &str, kind: ToolKind) -> Effective {
    let asks_by_preset = match policy.preset {
        ApprovalPreset::AskWrites => kind == ToolKind::Write,
        ApprovalPreset::AskAll | ApprovalPreset::AskAllEdit => true,
        ApprovalPreset::DontAsk => false,
    };
    let (ask, ask_source) = resolve(&policy.ask, tool, kind, asks_by_preset);
    let (edit, edit_source) = resolve(&policy.edit, tool, kind, policy.preset == ApprovalPreset::AskAllEdit);

    Effective { ask, edit, ask_source, edit_source }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(preset: ApprovalPreset) -> IntegrationPolicy {
        IntegrationPolicy { preset, ..Default::default() }
    }

    #[test]
    fn ask_before_writes_asks_for_writes_only() {
        let policy = preset(ApprovalPreset::AskWrites);

        assert!(effective(&policy, "send", ToolKind::Write).ask);
        assert!(!effective(&policy, "list", ToolKind::Read).ask);
        assert_eq!(effective(&policy, "list", ToolKind::Read).ask_source, Source::Preset);
    }

    #[test]
    fn ask_before_everything_asks_for_reads_too_without_editing() {
        let result = effective(&preset(ApprovalPreset::AskAll), "list", ToolKind::Read);

        assert!(result.ask);
        assert!(!result.edit);
    }

    #[test]
    fn the_edit_preset_asks_and_edits_every_call() {
        let result = effective(&preset(ApprovalPreset::AskAllEdit), "list", ToolKind::Read);

        assert!(result.ask && result.edit);
        assert_eq!(result.edit_source, Source::Preset);
    }

    #[test]
    fn dont_ask_asks_for_nothing_not_even_writes() {
        assert!(!effective(&preset(ApprovalPreset::DontAsk), "send", ToolKind::Write).ask);
    }

    #[test]
    fn a_type_rule_beats_the_preset() {
        let policy = IntegrationPolicy { ask: PolicyLevel { read: Some(true), ..Default::default() }, ..Default::default() };
        let result = effective(&policy, "list", ToolKind::Read);

        assert!(result.ask);
        assert_eq!(result.ask_source, Source::Type);
    }

    #[test]
    fn a_tool_rule_beats_the_type_rule() {
        let mut level = PolicyLevel { read: Some(false), ..Default::default() };
        level.tools.insert("list".to_string(), true);
        let result = effective(&IntegrationPolicy { ask: level, ..Default::default() }, "list", ToolKind::Read);

        assert!(result.ask);
        assert_eq!(result.ask_source, Source::Tool);
    }

    #[test]
    fn a_tool_can_stop_asking_and_still_be_held_for_editing() {
        let mut level = PolicyLevel::default();
        level.tools.insert("get_messages".to_string(), false);
        let policy = IntegrationPolicy { preset: ApprovalPreset::AskAllEdit, ask: level, ..Default::default() };
        let result = effective(&policy, "get_messages", ToolKind::Read);

        assert!(!result.ask);
        assert!(result.edit);
    }

    #[test]
    fn editing_is_resolved_separately_from_asking() {
        let policy = IntegrationPolicy { edit: PolicyLevel { write: Some(true), ..Default::default() }, preset: ApprovalPreset::DontAsk, ..Default::default() };
        let result = effective(&policy, "send", ToolKind::Write);

        assert!(result.edit);
        assert_eq!(result.edit_source, Source::Type);
        assert!(!result.ask);
    }
}
