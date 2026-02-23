import { useState, useRef, useEffect } from 'preact/hooks';
import { api } from '../lib/api';

interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
  metadata?: Record<string, unknown>;
}

interface SessionInfo {
  id: string;
  status: string;
  project_id?: string;
}

export function CreateApp() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [session, setSession] = useState<SessionInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [connected, setConnected] = useState(false);
  const messagesEnd = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);

  // Auto-scroll on new messages
  useEffect(() => {
    messagesEnd.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // Cleanup WebSocket on unmount
  useEffect(() => {
    return () => { wsRef.current?.close(); };
  }, []);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    if (!input.trim()) return;

    const userMsg = input.trim();
    setInput('');

    if (!session) {
      // First message — create the session
      setLoading(true);
      setMessages(prev => [...prev, { role: 'user', content: userMsg }]);

      try {
        const resp = await api.post<SessionInfo>('/api/create-app', {
          description: userMsg,
        });
        setSession(resp);
        setMessages(prev => [...prev, { role: 'system', content: `Session created (${resp.status}). Connecting...` }]);
        // Note: WebSocket streaming requires the session to be running with a pod.
        // For now we just show the session was created. A real implementation would
        // connect via WS once the pod is up.
        setConnected(true);
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : 'Failed to create session';
        setMessages(prev => [...prev, { role: 'system', content: `Error: ${msg}` }]);
      } finally {
        setLoading(false);
      }
    } else {
      // Follow-up message — send to the session
      setMessages(prev => [...prev, { role: 'user', content: userMsg }]);
      try {
        await api.post(`/api/sessions/${session.id}/message`, { content: userMsg });
      } catch {
        setMessages(prev => [...prev, { role: 'system', content: 'Failed to send message' }]);
      }
    }
  }

  return (
    <div style="display:flex;flex-direction:column;height:calc(100vh - 4rem)">
      <div style="padding:1rem 0;border-bottom:1px solid var(--border)">
        <h2 style="margin:0">Create New App</h2>
        <p class="text-muted text-sm" style="margin:0.25rem 0 0">
          Describe what you want to build. An AI agent will set up the project, pipeline, and infrastructure.
        </p>
      </div>

      {/* Messages area */}
      <div style="flex:1;overflow-y:auto;padding:1rem 0">
        {messages.length === 0 && (
          <div style="text-align:center;padding:3rem;color:var(--text-muted)">
            <p style="font-size:1.1rem;margin-bottom:0.5rem">What would you like to build?</p>
            <p class="text-sm">Examples: "A REST API with auth and a Postgres database", "A static blog with markdown", "A microservice in Go"</p>
          </div>
        )}
        {messages.map((msg, i) => (
          <div key={i} class={`chat-msg chat-msg-${msg.role}`} style={msgStyle(msg.role)}>
            <div class="chat-msg-role" style="font-weight:600;margin-bottom:0.25rem;font-size:0.85rem">
              {msg.role === 'user' ? 'You' : msg.role === 'assistant' ? 'Agent' : 'System'}
            </div>
            <div class="chat-msg-content" style="white-space:pre-wrap">{msg.content}</div>
          </div>
        ))}
        {loading && (
          <div style="padding:0.5rem 1rem;color:var(--text-muted);font-style:italic">
            Creating session...
          </div>
        )}
        <div ref={messagesEnd} />
      </div>

      {/* Input area */}
      <form onSubmit={handleSubmit} style="display:flex;gap:0.5rem;padding:1rem 0;border-top:1px solid var(--border)">
        <input
          type="text"
          class="input"
          style="flex:1"
          placeholder={session ? 'Send a follow-up message...' : 'Describe your app idea...'}
          value={input}
          onInput={(e) => setInput((e.target as HTMLInputElement).value)}
          disabled={loading}
          autoFocus
        />
        <button type="submit" class="btn btn-primary" disabled={loading || !input.trim()}>
          {session ? 'Send' : 'Create'}
        </button>
      </form>

      {/* Session info */}
      {session && (
        <div class="text-sm text-muted" style="padding-bottom:0.5rem">
          Session: {session.id.slice(0, 8)} | Status: {session.status}
          {session.project_id && (
            <span> | <a href={`/projects/${session.project_id}`}>View Project</a></span>
          )}
        </div>
      )}
    </div>
  );
}

function msgStyle(role: string): Record<string, string> {
  const base: Record<string, string> = { padding: '0.75rem 1rem', 'margin-bottom': '0.5rem', 'border-radius': '0.5rem' };
  if (role === 'user') {
    return { ...base, background: 'var(--bg-secondary)', 'margin-left': '2rem' };
  }
  if (role === 'assistant') {
    return { ...base, background: 'var(--bg-tertiary, var(--bg-secondary))', 'margin-right': '2rem' };
  }
  return { ...base, color: 'var(--text-muted)', 'font-style': 'italic', 'font-size': '0.85rem' };
}
