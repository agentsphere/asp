-- Extend agent_messages.role CHECK constraint to accept ProgressKind values
-- from the pub/sub persistence subscriber (text, thinking, tool_call, etc.)
-- in addition to the original chat-style roles (user, assistant, system, tool).
ALTER TABLE agent_messages DROP CONSTRAINT agent_messages_role_check;
ALTER TABLE agent_messages ADD CONSTRAINT agent_messages_role_check
    CHECK (role IN ('user', 'assistant', 'system', 'tool',
                    'text', 'thinking', 'tool_call', 'tool_result',
                    'milestone', 'error', 'completed'));
