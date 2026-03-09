//! NATS subject hierarchy for AstralMaris agent coordination
//!
//! All subjects follow the `am.` prefix convention. The `{project}` token
//! scopes subjects to a specific AstralMaris project (e.g. `mingqiao`,
//! `buildermoon`, `echoessence`). Set via `project` in `ming-qiao.toml`.
//!
//! ## Full subject hierarchy
//!
//! ```text
//! am.agent.{agent}.presence                          — Heartbeat (core NATS, ephemeral)
//! am.agent.{agent}.task.{project}.assigned           — Task assigned to agent
//! am.agent.{agent}.task.{project}.started            — Agent started working on task
//! am.agent.{agent}.task.{project}.update             — Progress update on task
//! am.agent.{agent}.task.{project}.complete           — Task completed
//! am.agent.{agent}.task.{project}.blocked            — Agent blocked on task
//! am.agent.{agent}.message.{project}                  — Message notification hint (core NATS, ephemeral)
//! am.agent.{agent}.notes.{project}                   — Session notes
//! am.events.{project}                                — Per-project event broadcast
//! am.observe.{type}.{target}                         — Observations (target = project or topic)
//! am.council.announce                                — System-wide announcements (not project-scoped)
//! ```
//!
//! ## Subscribe patterns
//!
//! ```text
//! am.agent.*.presence                                — All agents' heartbeats
//! am.agent.{agent}.task.{project}.>                  — Everything one agent does on a project
//! am.agent.*.task.{project}.>                        — All agents on a project
//! am.agent.*.message.{project}                        — All agents' message notifications on a project
//! am.agent.*.notes.>                                 — All agents' session notes
//! am.events.>                                        — All projects' event broadcasts
//! am.observe.>                                       — All observations
//! am.council.>                                       — All council-wide messages
//! ```

/// Subject builder for a specific agent on a specific project.
///
/// Holds the agent name and project token, provides methods that return
/// fully-qualified NATS subjects. Keeps the subject hierarchy in one place
/// and makes it testable without running NATS.
#[derive(Debug, Clone)]
pub struct AgentSubjects {
    agent: String,
    project: String,
}

impl AgentSubjects {
    /// Create a subject builder for an agent on a project.
    ///
    /// The project token should be lowercase, no hyphens (e.g., `"mingqiao"`).
    pub fn new(agent: impl Into<String>, project: impl Into<String>) -> Self {
        Self {
            agent: agent.into(),
            project: project.into(),
        }
    }

    /// Get the agent name.
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// Get the project token.
    pub fn project(&self) -> &str {
        &self.project
    }

    // ========================================================================
    // Presence (core NATS — ephemeral, no JetStream)
    // ========================================================================

    /// This agent's presence heartbeat subject.
    ///
    /// `am.agent.{agent}.presence`
    pub fn presence(&self) -> String {
        format!("am.agent.{}.presence", self.agent)
    }

    // ========================================================================
    // Events broadcast (core NATS — ephemeral, cross-process sync)
    // ========================================================================

    /// Shared event broadcast subject for cross-process Indexer sync.
    ///
    /// Project-scoped (not agent-scoped) since all processes share the same
    /// event stream. SurrealDB + hydration provides durability; this is
    /// fire-and-forget for real-time sync.
    ///
    /// `am.events.{project}`
    pub fn events(&self) -> String {
        format!("am.events.{}", self.project)
    }

    // ========================================================================
    // Task coordination (JetStream — persistent, work queue)
    // ========================================================================

    /// Task assigned to this agent.
    ///
    /// `am.agent.{agent}.task.{project}.assigned`
    pub fn task_assigned(&self) -> String {
        format!("am.agent.{}.task.{}.assigned", self.agent, self.project)
    }

    /// This agent started working on a task.
    ///
    /// `am.agent.{agent}.task.{project}.started`
    pub fn task_started(&self) -> String {
        format!("am.agent.{}.task.{}.started", self.agent, self.project)
    }

    /// Progress update from this agent.
    ///
    /// `am.agent.{agent}.task.{project}.update`
    pub fn task_update(&self) -> String {
        format!("am.agent.{}.task.{}.update", self.agent, self.project)
    }

    /// Task completed by this agent.
    ///
    /// `am.agent.{agent}.task.{project}.complete`
    pub fn task_complete(&self) -> String {
        format!("am.agent.{}.task.{}.complete", self.agent, self.project)
    }

    /// This agent is blocked on a task.
    ///
    /// `am.agent.{agent}.task.{project}.blocked`
    pub fn task_blocked(&self) -> String {
        format!("am.agent.{}.task.{}.blocked", self.agent, self.project)
    }

    /// Wildcard for all task events from this agent on this project.
    ///
    /// `am.agent.{agent}.task.{project}.>`
    pub fn task_wildcard(&self) -> String {
        format!("am.agent.{}.task.{}.>", self.agent, self.project)
    }

    // ========================================================================
    // Message notifications (core NATS — ephemeral, notification hint)
    // ========================================================================

    /// Notification hint when a message is sent to this agent.
    ///
    /// Core NATS (ephemeral, not JetStream). Actual message content lives in
    /// SurrealDB; this is a "you have mail" nudge for cross-process MCP servers.
    ///
    /// `am.agent.{agent}.message.{project}`
    pub fn message(&self) -> String {
        format!("am.agent.{}.message.{}", self.agent, self.project)
    }

    /// Subscribe to message notifications for all agents on a project.
    ///
    /// `am.agent.*.message.{project}`
    pub fn all_agents_messages(project: &str) -> String {
        format!("am.agent.*.message.{}", project)
    }

    // ========================================================================
    // Message events (JetStream — durable delivery, Phase 2)
    // ========================================================================

    /// JetStream subject for a message event addressed to a specific agent.
    ///
    /// Used by the HTTP server to publish message events to the AGENT_MESSAGES
    /// stream after writing to SurrealDB. The consumer on the HTTP server
    /// ingests these for cross-process Indexer consistency.
    ///
    /// `am.agent.{to_agent}.msg.sent`
    pub fn message_event(to_agent: &str) -> String {
        format!("am.agent.{}.msg.sent", to_agent)
    }

    /// Subscribe pattern for all agent message events.
    ///
    /// `am.agent.*.msg.>`
    pub fn all_agents_message_events() -> String {
        "am.agent.*.msg.>".to_string()
    }

    // ========================================================================
    // Session notes (JetStream — persistent, 30-day retention)
    // ========================================================================

    /// Session notes from this agent on this project.
    ///
    /// `am.agent.{agent}.notes.{project}`
    pub fn notes(&self) -> String {
        format!("am.agent.{}.notes.{}", self.agent, self.project)
    }

    // ========================================================================
    // Subscribe patterns (wildcards for receiving from other agents)
    // ========================================================================

    /// Subscribe to all agents' presence heartbeats.
    ///
    /// `am.agent.*.presence`
    pub fn all_agents_presence() -> String {
        "am.agent.*.presence".to_string()
    }

    /// Subscribe to all agents' task events on this project.
    ///
    /// `am.agent.*.task.{project}.>`
    pub fn all_agents_task_wildcard(project: &str) -> String {
        format!("am.agent.*.task.{}.>", project)
    }

    /// Subscribe to all agents' session notes (any project).
    ///
    /// `am.agent.*.notes.>`
    pub fn all_agents_notes() -> String {
        "am.agent.*.notes.>".to_string()
    }

    /// Subscribe to all agents' session notes on a specific project.
    ///
    /// `am.agent.*.notes.{project}`
    pub fn all_agents_notes_for_project(project: &str) -> String {
        format!("am.agent.*.notes.{}", project)
    }

    /// Subscribe to everything a specific agent does (any project).
    ///
    /// `am.agent.{agent}.>`
    pub fn everything_from_agent(agent: &str) -> String {
        format!("am.agent.{}.>", agent)
    }

    // ========================================================================
    // Council-wide (core NATS — not project-scoped)
    // ========================================================================

    /// System-wide announcement subject for cross-project messages.
    ///
    /// Used for new agent introductions, policy changes, and coordination
    /// that isn't scoped to any single project.
    ///
    /// `am.council.announce`
    pub fn council_announce() -> String {
        "am.council.announce".to_string()
    }

    // ========================================================================
    // Echo suppression helpers
    // ========================================================================

    /// The subject prefix for this agent, used for echo suppression.
    ///
    /// Messages with subjects starting with this prefix were published by us.
    /// `am.agent.{agent}.`
    pub fn own_prefix(&self) -> String {
        format!("am.agent.{}.", self.agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subjects() -> AgentSubjects {
        AgentSubjects::new("aleph", "mingqiao")
    }

    // ========================================================================
    // am. prefix convention
    // ========================================================================

    #[test]
    fn test_all_subjects_start_with_am_prefix() {
        let s = subjects();
        let all = vec![
            s.presence(),
            s.events(),
            s.message(),
            s.task_assigned(),
            s.task_started(),
            s.task_update(),
            s.task_complete(),
            s.task_blocked(),
            s.task_wildcard(),
            s.notes(),
        ];
        for subject in &all {
            assert!(
                subject.starts_with("am."),
                "Subject '{}' does not start with 'am.'",
                subject
            );
        }

        // Static subscribe patterns too
        assert!(AgentSubjects::all_agents_presence().starts_with("am."));
        assert!(AgentSubjects::all_agents_task_wildcard("mingqiao").starts_with("am."));
        assert!(AgentSubjects::all_agents_messages("mingqiao").starts_with("am."));
        assert!(AgentSubjects::all_agents_notes().starts_with("am."));
        assert!(AgentSubjects::council_announce().starts_with("am."));
        assert!(AgentSubjects::message_event("thales").starts_with("am."));
        assert!(AgentSubjects::all_agents_message_events().starts_with("am."));
    }

    // ========================================================================
    // Presence
    // ========================================================================

    #[test]
    fn test_presence_subject() {
        assert_eq!(subjects().presence(), "am.agent.aleph.presence");
    }

    #[test]
    fn test_all_agents_presence() {
        assert_eq!(AgentSubjects::all_agents_presence(), "am.agent.*.presence");
    }

    // ========================================================================
    // Events broadcast
    // ========================================================================

    #[test]
    fn test_events_subject() {
        assert_eq!(subjects().events(), "am.events.mingqiao");
    }

    // ========================================================================
    // Task coordination
    // ========================================================================

    #[test]
    fn test_task_subjects() {
        let s = subjects();
        assert_eq!(s.task_assigned(), "am.agent.aleph.task.mingqiao.assigned");
        assert_eq!(s.task_started(), "am.agent.aleph.task.mingqiao.started");
        assert_eq!(s.task_update(), "am.agent.aleph.task.mingqiao.update");
        assert_eq!(s.task_complete(), "am.agent.aleph.task.mingqiao.complete");
        assert_eq!(s.task_blocked(), "am.agent.aleph.task.mingqiao.blocked");
    }

    #[test]
    fn test_task_wildcard() {
        assert_eq!(
            subjects().task_wildcard(),
            "am.agent.aleph.task.mingqiao.>"
        );
    }

    #[test]
    fn test_all_agents_task_wildcard() {
        assert_eq!(
            AgentSubjects::all_agents_task_wildcard("mingqiao"),
            "am.agent.*.task.mingqiao.>"
        );
    }

    // ========================================================================
    // Message notifications
    // ========================================================================

    #[test]
    fn test_message_subject() {
        assert_eq!(subjects().message(), "am.agent.aleph.message.mingqiao");
    }

    #[test]
    fn test_all_agents_messages() {
        assert_eq!(
            AgentSubjects::all_agents_messages("mingqiao"),
            "am.agent.*.message.mingqiao"
        );
    }

    #[test]
    fn test_message_subject_scoped_to_recipient() {
        let aleph = AgentSubjects::new("aleph", "mingqiao");
        let luban = AgentSubjects::new("luban", "mingqiao");

        // Messages to different agents produce different subjects
        assert_ne!(aleph.message(), luban.message());
        assert_eq!(luban.message(), "am.agent.luban.message.mingqiao");
    }

    // ========================================================================
    // Message events (JetStream)
    // ========================================================================

    #[test]
    fn test_message_event_subject() {
        assert_eq!(
            AgentSubjects::message_event("thales"),
            "am.agent.thales.msg.sent"
        );
    }

    #[test]
    fn test_all_agents_message_events() {
        assert_eq!(
            AgentSubjects::all_agents_message_events(),
            "am.agent.*.msg.>"
        );
    }

    // ========================================================================
    // Session notes
    // ========================================================================

    #[test]
    fn test_notes_subject() {
        assert_eq!(subjects().notes(), "am.agent.aleph.notes.mingqiao");
    }

    #[test]
    fn test_all_agents_notes() {
        assert_eq!(AgentSubjects::all_agents_notes(), "am.agent.*.notes.>");
    }

    #[test]
    fn test_all_agents_notes_for_project() {
        assert_eq!(
            AgentSubjects::all_agents_notes_for_project("mingqiao"),
            "am.agent.*.notes.mingqiao"
        );
    }

    // ========================================================================
    // Council-wide
    // ========================================================================

    #[test]
    fn test_council_announce() {
        assert_eq!(AgentSubjects::council_announce(), "am.council.announce");
    }

    // ========================================================================
    // Cross-agent patterns
    // ========================================================================

    #[test]
    fn test_everything_from_agent() {
        assert_eq!(
            AgentSubjects::everything_from_agent("luban"),
            "am.agent.luban.>"
        );
    }

    // ========================================================================
    // Echo suppression
    // ========================================================================

    #[test]
    fn test_own_prefix() {
        assert_eq!(subjects().own_prefix(), "am.agent.aleph.");
    }

    #[test]
    fn test_echo_suppression_matches_own_subjects() {
        let s = subjects();
        let prefix = s.own_prefix();

        // All our subjects should start with our prefix
        assert!(s.presence().starts_with(&prefix));
        assert!(s.message().starts_with(&prefix));
        assert!(s.task_assigned().starts_with(&prefix));
        assert!(s.notes().starts_with(&prefix));
    }

    #[test]
    fn test_echo_suppression_does_not_match_other_agents() {
        let aleph = AgentSubjects::new("aleph", "mingqiao");
        let luban = AgentSubjects::new("luban", "mingqiao");

        // Aleph's prefix should not match Luban's subjects
        let aleph_prefix = aleph.own_prefix();
        assert!(!luban.task_assigned().starts_with(&aleph_prefix));
        assert!(!luban.presence().starts_with(&aleph_prefix));
        assert!(!luban.notes().starts_with(&aleph_prefix));
    }

    // ========================================================================
    // Different agents, same project
    // ========================================================================

    #[test]
    fn test_symmetric_interface() {
        let aleph = AgentSubjects::new("aleph", "mingqiao");
        let luban = AgentSubjects::new("luban", "mingqiao");

        // Aleph assigning a task to Luban uses Luban's subject
        let assign_subject = luban.task_assigned();
        assert_eq!(assign_subject, "am.agent.luban.task.mingqiao.assigned");

        // Luban subscribes to their own task wildcard
        let luban_sub = luban.task_wildcard();
        assert_eq!(luban_sub, "am.agent.luban.task.mingqiao.>");

        // Both agents can see everything on the project
        let project_sub = AgentSubjects::all_agents_task_wildcard("mingqiao");
        assert_eq!(project_sub, "am.agent.*.task.mingqiao.>");

        // Verify agent names
        assert_eq!(aleph.agent(), "aleph");
        assert_eq!(luban.agent(), "luban");
        assert_eq!(aleph.project(), "mingqiao");
    }
}
