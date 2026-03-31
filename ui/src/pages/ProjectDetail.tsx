import { useState, useEffect } from 'preact/hooks';
import { api, qs, type ListResponse } from '../lib/api';
import type { Project, Issue, MergeRequest, Pipeline, Deployment, Webhook, TreeEntry, BlobResponse, BranchInfo, PreviewDeployment, Secret, AgentSession, IframePanel, LogEntry, UiPreviewArtifact, UiPreviewFile, UiPreviewConfig, UiPreviewGroup, UiPreviewItem } from '../lib/types';
import { timeAgo } from '../lib/format';
import { Badge } from '../components/Badge';
import { StatusDot } from '../components/StatusDot';
import { Pagination } from '../components/Pagination';
import { Modal } from '../components/Modal';
import { FilterBar } from '../components/FilterBar';
import { AgentChatPanel } from '../components/AgentChatPanel';
import { Sessions } from './Sessions';

interface Props { id?: string; tab?: string; }

const TABS = ['files', 'issues', 'mrs', 'builds', 'ui', 'deployments', 'sessions', 'logs', 'skills', 'webhooks', 'settings'];

export function ProjectDetail({ id, tab }: Props) {
  const [project, setProject] = useState<Project | null>(null);
  const [chatOpen, setChatOpen] = useState(false);
  const [activeSession, setActiveSession] = useState<AgentSession | null>(null);
  const [deployments, setDeployments] = useState<Deployment[]>([]);
  const [iframes, setIframes] = useState<IframePanel[]>([]);
  const [deployIframes, setDeployIframes] = useState<IframePanel[]>([]);
  const [activePreviewIdx, setActivePreviewIdx] = useState(0);
  const [progressText, setProgressText] = useState<string | null>(null);
  const currentTab = tab || 'files';

  useEffect(() => {
    if (id) api.get<Project>(`/api/projects/${id}`).then(setProject).catch(e => console.warn(e));
  }, [id]);

  useEffect(() => {
    if (!id) return;
    api.get<ListResponse<AgentSession>>(`/api/projects/${id}/sessions?status=running&limit=1`)
      .then(r => { if (r.items.length > 0) setActiveSession(r.items[0]); })
      .catch(e => console.warn(e));
    api.get<ListResponse<Deployment>>(`/api/projects/${id}/deployments?limit=5`)
      .then(r => setDeployments(r.items))
      .catch(e => console.warn(e));
  }, [id]);

  // Fetch iframes + progress for active session
  useEffect(() => {
    if (!activeSession || !id) return;
    api.get<{ items: IframePanel[] }>(`/api/projects/${id}/sessions/${activeSession.id}/iframes`)
      .then(r => setIframes(r.items)).catch(() => setIframes([]));
    api.get<{ message: string }>(`/api/projects/${id}/sessions/${activeSession.id}/progress`)
      .then(r => setProgressText(r.message)).catch(e => console.warn(e));
  }, [activeSession, id]);

  // Fetch deploy iframes when no session iframes
  useEffect(() => {
    if (!id || iframes.length > 0) return;
    api.get<IframePanel[]>(`/api/projects/${id}/deploy-preview/iframes`)
      .then(setDeployIframes)
      .catch(() => setDeployIframes([]));
  }, [id, iframes.length]);

  if (!project) return <div class="empty-state">Loading...</div>;

  const displayName = project.display_name || project.name;
  const initial = displayName.charAt(0).toUpperCase();
  const activeIframes = iframes.length > 0 ? iframes : deployIframes;
  const clampedIdx = Math.min(activePreviewIdx, Math.max(0, activeIframes.length - 1));
  const currentIframe = activeIframes[clampedIdx];
  const previewUrl = currentIframe?.preview_url ?? null;
  const hasActiveSession = !!activeSession;

  // Group deployments by env for the header
  const envSummary = new Map<string, string>();
  for (const d of deployments) {
    if (!envSummary.has(d.environment)) {
      envSummary.set(d.environment, d.current_status);
    }
  }

  const statusColor = (s: string): string => {
    if (s === 'healthy' || s === 'success' || s === 'running') return 'var(--success)';
    if (s === 'degraded' || s === 'syncing' || s === 'pending') return 'var(--warning)';
    if (s === 'failure' || s === 'failed' || s === 'error') return 'var(--danger)';
    return 'var(--text-muted)';
  };

  return (
    <div>
      <div class="flex-between mb-md">
        <div>
          <h2>{displayName}</h2>
          {project.description && <p class="text-muted text-sm mt-sm">{project.description}</p>}
        </div>
        <div class="flex gap-sm" style="align-items:center">
          <button class="btn btn-sm btn-primary" onClick={() => setChatOpen(true)}>
            {hasActiveSession ? '\u25CF Agent' : 'Agent'}
          </button>
          <Badge status={project.visibility} />
        </div>
      </div>

      {/* Preview + status header */}
      <div class="project-header-card">
        <div class="project-header-preview" style="position:relative">
          {previewUrl ? (
            <iframe src={previewUrl} tabIndex={-1} loading="lazy" sandbox="allow-scripts allow-same-origin allow-forms allow-popups" />
          ) : (
            <div class="project-header-preview-placeholder">{initial}</div>
          )}
          {activeIframes.length > 1 && (
            <div class="project-card-preview-dots">
              {activeIframes.map((_, i) => (
                <button key={i} class={`preview-dot ${i === clampedIdx ? 'active' : ''}`}
                  onClick={() => setActivePreviewIdx(i)} />
              ))}
            </div>
          )}
        </div>
        <div class="project-header-status">
          {Array.from(envSummary.entries()).map(([env, status]) => (
            <div key={env} class="project-header-status-row">
              <span class="project-header-status-label" style="text-transform:capitalize">{env}</span>
              <span class="project-header-status-value">
                <span class="status-dot" style={`background:${statusColor(status)}`} />
                {status}
              </span>
            </div>
          ))}
          {envSummary.size === 0 && (
            <div class="project-header-status-row">
              <span class="project-header-status-label">Deploy</span>
              <span style="color:var(--text-muted)">Not deployed</span>
            </div>
          )}
          <div class="project-header-status-row">
            <span class="project-header-status-label">Session</span>
            {activeSession ? (
              <span class="project-header-status-value">
                <span class="status-dot" style="background:var(--success)" />
                {progressText || 'Running...'}
              </span>
            ) : (
              <span style="color:var(--text-muted)">No active session</span>
            )}
          </div>
        </div>
      </div>

      {/* Glass tab bar */}
      <div class="tabs-glass">
        {TABS.map(t => (
          <a key={t} class={`tab${currentTab === t ? ' active' : ''}`}
            href={`/projects/${id}/${t}`}>{t === 'mrs' ? 'MRs' : t === 'ui' ? 'UI' : t === 'sessions' ? 'Sessions' : t[0].toUpperCase() + t.slice(1)}</a>
        ))}
      </div>
      {currentTab === 'files' && <FilesTab projectId={id!} defaultBranch={project.default_branch} />}
      {currentTab === 'issues' && <IssuesTab projectId={id!} />}
      {currentTab === 'mrs' && <MRsTab projectId={id!} />}
      {currentTab === 'builds' && <BuildsTab projectId={id!} />}
      {currentTab === 'ui' && <UiPreviewsTab projectId={id!} defaultBranch={project.default_branch} />}
      {currentTab === 'deployments' && <DeploymentsTab projectId={id!} />}
      {currentTab === 'sessions' && <Sessions projectId={id!} />}
      {currentTab === 'logs' && <ProjectLogs projectId={id!} />}
      {currentTab === 'skills' && <SkillsTab projectId={id!} />}
      {currentTab === 'webhooks' && <WebhooksTab projectId={id!} />}
      {currentTab === 'settings' && <SettingsTab project={project} onUpdate={setProject} />}
      <AgentChatPanel projectId={id!} open={chatOpen} onClose={() => setChatOpen(false)} />
    </div>
  );
}

function ProjectLogs({ projectId }: { projectId: string }) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [filters, setFilters] = useState<Record<string, string>>({ range: '24h', level: '', source: '' });
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);

  const load = () => {
    setLoading(true);
    const params: Record<string, string | number> = { limit: 50, offset };
    if (filters.range) params.range = filters.range;
    if (filters.level) params.level = filters.level;
    if (filters.source) params.source = filters.source;
    if (filters.q) params.q = filters.q;

    api.get<ListResponse<LogEntry>>(`/api/projects/${projectId}/logs${qs(params)}`)
      .then(r => { setLogs(r.items); setTotal(r.total); })
      .catch(e => console.warn(e))
      .finally(() => setLoading(false));
  };

  useEffect(load, [offset, projectId]);

  const toggleExpand = (id: string) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const formatTime = (ts: string) => {
    const d = new Date(ts);
    return d.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
  };

  const LEVEL_CLASSES: Record<string, string> = {
    error: 'log-level-error', warn: 'log-level-warn', info: 'log-level-info',
    debug: 'log-level-debug', trace: 'log-level-trace',
  };

  return (
    <div>
      <FilterBar filters={[
        { key: 'range', label: 'Time range', type: 'select', options: [
          { value: '1h', label: 'Last 1 hour' }, { value: '6h', label: 'Last 6 hours' },
          { value: '24h', label: 'Last 24 hours' }, { value: '7d', label: 'Last 7 days' },
        ] },
        { key: 'level', label: 'Level', type: 'select', options: [
          { value: '', label: 'All levels' }, { value: 'error', label: 'Error' },
          { value: 'warn', label: 'Warn' }, { value: 'info', label: 'Info' },
        ] },
        { key: 'source', label: 'Source', type: 'select', options: [
          { value: '', label: 'All sources' }, { value: 'system', label: 'System' },
          { value: 'api', label: 'API' }, { value: 'session', label: 'Session' },
          { value: 'external', label: 'External' },
        ] },
        { key: 'q', label: 'Search', type: 'text', placeholder: 'Full-text search...' },
      ]} values={filters} onChange={setFilters} onApply={() => { setOffset(0); load(); }} />
      <div class="card" style="margin-top:1rem">
        {loading ? (
          <div class="empty-state">Loading...</div>
        ) : logs.length === 0 ? (
          <div class="empty-state">No log entries found</div>
        ) : (
          <div class="log-list">
            {logs.map(entry => (
              <div key={entry.id} class="log-entry" onClick={() => toggleExpand(entry.id)}>
                <div class="log-entry-row">
                  <span class="log-time mono text-xs">{formatTime(entry.timestamp)}</span>
                  <span class={`log-level ${LEVEL_CLASSES[entry.level.toLowerCase()] || ''}`}>
                    {entry.level.toUpperCase().padEnd(5)}
                  </span>
                  <span class="log-source text-xs" style="opacity:0.6">{entry.source}</span>
                  <span class="log-service text-xs">{entry.service}</span>
                  <span class="log-message">{entry.message}</span>
                </div>
                {expanded.has(entry.id) && entry.attributes && (
                  <div class="log-attributes">
                    <pre class="log-viewer" style="max-height:200px;margin-top:0.5rem">
                      {JSON.stringify(entry.attributes, null, 2)}
                    </pre>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
        <Pagination total={total} limit={50} offset={offset} onChange={setOffset} />
      </div>
    </div>
  );
}

function FilesTab({ projectId, defaultBranch }: { projectId: string; defaultBranch: string }) {
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [gitRef, setRef] = useState(defaultBranch);
  const [path, setPath] = useState('');
  const [entries, setEntries] = useState<TreeEntry[]>([]);
  const [blob, setBlob] = useState<BlobResponse | null>(null);

  useEffect(() => {
    api.get<BranchInfo[]>(`/api/projects/${projectId}/branches`).then(setBranches).catch(e => console.warn(e));
  }, [projectId]);

  useEffect(() => {
    setBlob(null);
    api.get<TreeEntry[]>(`/api/projects/${projectId}/tree${qs({ ref: gitRef, path })}`)
      .then(setEntries)
      .catch(() => setEntries([]));
  }, [projectId, gitRef, path]);

  const openEntry = (entry: TreeEntry) => {
    if (entry.entry_type === 'tree') {
      setPath(path ? `${path}/${entry.name}` : entry.name);
    } else {
      const filePath = path ? `${path}/${entry.name}` : entry.name;
      api.get<BlobResponse>(`/api/projects/${projectId}/blob${qs({ ref: gitRef, path: filePath })}`)
        .then(setBlob).catch(e => console.warn(e));
    }
  };

  if (blob) {
    return (
      <div class="card">
        <div class="flex-between mb-md">
          <span class="mono text-sm">{blob.path}</span>
          <button class="btn btn-sm" onClick={() => setBlob(null)}>Back</button>
        </div>
        <pre class="log-viewer">{blob.encoding === 'base64' ? atob(blob.content) : blob.content}</pre>
      </div>
    );
  }

  return (
    <div class="card">
      <div class="flex gap-sm mb-md">
        <select class="input" style="width:auto" value={gitRef}
          onChange={(e) => { setRef((e.target as HTMLSelectElement).value); setPath(''); }}>
          {branches.map(b => <option key={b.name} value={b.name}>{b.name}</option>)}
        </select>
        {path && (
          <button class="btn btn-sm" onClick={() => {
            const parts = path.split('/');
            parts.pop();
            setPath(parts.join('/'));
          }}>.. (up)</button>
        )}
        {path && <span class="mono text-sm text-muted">{path}/</span>}
      </div>
      {entries.length === 0 ? (
        <div class="empty-state">No files</div>
      ) : (
        entries.sort((a, b) => (a.entry_type === b.entry_type ? a.name.localeCompare(b.name) : a.entry_type === 'tree' ? -1 : 1))
          .map(e => (
            <div key={e.name} class="tree-entry" onClick={() => openEntry(e)}>
              <span class="tree-icon">{e.entry_type === 'tree' ? '/' : ' '}</span>
              <span>{e.name}</span>
              {e.size != null && <span class="text-muted text-xs" style="margin-left:auto">{e.size}</span>}
            </div>
          ))
      )}
    </div>
  );
}

function IssuesTab({ projectId }: { projectId: string }) {
  const [issues, setIssues] = useState<Issue[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [status, setStatus] = useState('open');
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ title: '', body: '' });
  const [error, setError] = useState('');

  const load = () => {
    api.get<ListResponse<Issue>>(`/api/projects/${projectId}/issues${qs({ limit: 20, offset, status })}`)
      .then(r => { setIssues(r.items); setTotal(r.total); }).catch(e => console.warn(e));
  };
  useEffect(load, [projectId, offset, status]);

  const create = async (e: Event) => {
    e.preventDefault();
    try {
      await api.post(`/api/projects/${projectId}/issues`, form);
      setShowCreate(false);
      setForm({ title: '', body: '' });
      load();
    } catch (err: any) { setError(err.message); }
  };

  return (
    <div>
      <div class="flex-between mb-md">
        <div class="flex gap-sm">
          {['open', 'closed'].map(s => (
            <button key={s} class={`btn btn-sm${status === s ? ' btn-primary' : ''}`}
              onClick={() => { setStatus(s); setOffset(0); }}>{s}</button>
          ))}
        </div>
        <button class="btn btn-primary btn-sm" onClick={() => setShowCreate(true)}>New Issue</button>
      </div>
      <div class="card">
        {issues.length === 0 ? <div class="empty-state">No issues</div> : (
          <table class="table">
            <thead><tr><th>#</th><th>Title</th><th>Status</th><th>Created</th></tr></thead>
            <tbody>
              {issues.map(i => (
                <tr key={i.id} class="table-link" onClick={() => { window.location.href = `/projects/${projectId}/issues/${i.number}`; }}>
                  <td class="text-muted">{i.number}</td>
                  <td>{i.title}</td>
                  <td><Badge status={i.status} /></td>
                  <td class="text-muted text-sm">{timeAgo(i.created_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <Pagination total={total} limit={20} offset={offset} onChange={setOffset} />
      </div>
      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="New Issue">
        <form onSubmit={create}>
          <div class="form-group">
            <label>Title</label>
            <input class="input" required value={form.title}
              onInput={(e) => setForm({ ...form, title: (e.target as HTMLInputElement).value })} />
          </div>
          <div class="form-group">
            <label>Body (markdown)</label>
            <textarea class="input" value={form.body}
              onInput={(e) => setForm({ ...form, body: (e.target as HTMLTextAreaElement).value })} />
          </div>
          {error && <div class="error-msg">{error}</div>}
          <div class="modal-actions">
            <button type="button" class="btn" onClick={() => setShowCreate(false)}>Cancel</button>
            <button type="submit" class="btn btn-primary">Create</button>
          </div>
        </form>
      </Modal>
    </div>
  );
}

function MRsTab({ projectId }: { projectId: string }) {
  const [mrs, setMrs] = useState<MergeRequest[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [status, setStatus] = useState('open');
  const [showCreate, setShowCreate] = useState(false);
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [form, setForm] = useState({ source_branch: '', target_branch: 'main', title: '', body: '' });
  const [error, setError] = useState('');

  const load = () => {
    api.get<ListResponse<MergeRequest>>(`/api/projects/${projectId}/merge-requests${qs({ limit: 20, offset, status })}`)
      .then(r => { setMrs(r.items); setTotal(r.total); }).catch(e => console.warn(e));
  };
  useEffect(load, [projectId, offset, status]);

  const openCreate = () => {
    api.get<BranchInfo[]>(`/api/projects/${projectId}/branches`).then(setBranches).catch(e => console.warn(e));
    setShowCreate(true);
  };

  const create = async (e: Event) => {
    e.preventDefault();
    try {
      await api.post(`/api/projects/${projectId}/merge-requests`, form);
      setShowCreate(false);
      setForm({ source_branch: '', target_branch: 'main', title: '', body: '' });
      load();
    } catch (err: any) { setError(err.message); }
  };

  return (
    <div>
      <div class="flex-between mb-md">
        <div class="flex gap-sm">
          {['open', 'closed', 'merged'].map(s => (
            <button key={s} class={`btn btn-sm${status === s ? ' btn-primary' : ''}`}
              onClick={() => { setStatus(s); setOffset(0); }}>{s}</button>
          ))}
        </div>
        <button class="btn btn-primary btn-sm" onClick={openCreate}>New MR</button>
      </div>
      <div class="card">
        {mrs.length === 0 ? <div class="empty-state">No merge requests</div> : (
          <table class="table">
            <thead><tr><th>#</th><th>Title</th><th>Branches</th><th>Status</th><th>Created</th></tr></thead>
            <tbody>
              {mrs.map(m => (
                <tr key={m.id} class="table-link" onClick={() => { window.location.href = `/projects/${projectId}/merge-requests/${m.number}`; }}>
                  <td class="text-muted">{m.number}</td>
                  <td>{m.title}</td>
                  <td class="mono text-xs">{m.source_branch} → {m.target_branch}</td>
                  <td><Badge status={m.status} /></td>
                  <td class="text-muted text-sm">{timeAgo(m.created_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <Pagination total={total} limit={20} offset={offset} onChange={setOffset} />
      </div>
      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="New Merge Request">
        <form onSubmit={create}>
          <div class="form-group">
            <label>Source branch</label>
            <select class="input" value={form.source_branch}
              onChange={(e) => setForm({ ...form, source_branch: (e.target as HTMLSelectElement).value })}>
              <option value="">Select...</option>
              {branches.map(b => <option key={b.name} value={b.name}>{b.name}</option>)}
            </select>
          </div>
          <div class="form-group">
            <label>Target branch</label>
            <select class="input" value={form.target_branch}
              onChange={(e) => setForm({ ...form, target_branch: (e.target as HTMLSelectElement).value })}>
              {branches.map(b => <option key={b.name} value={b.name}>{b.name}</option>)}
            </select>
          </div>
          <div class="form-group">
            <label>Title</label>
            <input class="input" required value={form.title}
              onInput={(e) => setForm({ ...form, title: (e.target as HTMLInputElement).value })} />
          </div>
          <div class="form-group">
            <label>Description</label>
            <textarea class="input" value={form.body}
              onInput={(e) => setForm({ ...form, body: (e.target as HTMLTextAreaElement).value })} />
          </div>
          {error && <div class="error-msg">{error}</div>}
          <div class="modal-actions">
            <button type="button" class="btn" onClick={() => setShowCreate(false)}>Cancel</button>
            <button type="submit" class="btn btn-primary">Create</button>
          </div>
        </form>
      </Modal>
    </div>
  );
}

function BuildsTab({ projectId }: { projectId: string }) {
  const [pipelines, setPipelines] = useState<Pipeline[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);

  useEffect(() => {
    api.get<ListResponse<Pipeline>>(`/api/projects/${projectId}/pipelines${qs({ limit: 20, offset })}`)
      .then(r => { setPipelines(r.items); setTotal(r.total); }).catch(e => console.warn(e));
  }, [projectId, offset]);

  return (
    <div class="card">
      {pipelines.length === 0 ? <div class="empty-state">No pipelines</div> : (
        <table class="table">
          <thead><tr><th>Ref</th><th>Trigger</th><th>Status</th><th>Created</th></tr></thead>
          <tbody>
            {pipelines.map(p => (
              <tr key={p.id} class="table-link" onClick={() => { window.location.href = `/projects/${projectId}/pipelines/${p.id}`; }}>
                <td class="mono text-sm">{p.git_ref}</td>
                <td class="text-sm">{p.trigger}</td>
                <td><Badge status={p.status} /></td>
                <td class="text-muted text-sm">{timeAgo(p.created_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <Pagination total={total} limit={20} offset={offset} onChange={setOffset} />
    </div>
  );
}

interface UiPreviewsCompareResponse {
  base: UiPreviewArtifact[];
  head: UiPreviewArtifact[];
}

function UiPreviewsTab({ projectId, defaultBranch }: { projectId: string; defaultBranch: string }) {
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [branch, setBranch] = useState(defaultBranch);
  const [typeFilter, setTypeFilter] = useState<string>('all');
  const [artifacts, setArtifacts] = useState<UiPreviewArtifact[]>([]);
  const [loading, setLoading] = useState(true);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [metaFilter, setMetaFilter] = useState<{ key: string; value: string } | null>(null);
  const [lightbox, setLightbox] = useState<{ file: UiPreviewFile; artifact: UiPreviewArtifact; item: UiPreviewItem | null } | null>(null);
  const [compareEnabled, setCompareEnabled] = useState(false);
  const [compareBranch, setCompareBranch] = useState('');
  const [compareData, setCompareData] = useState<UiPreviewsCompareResponse | null>(null);

  useEffect(() => {
    api.get<BranchInfo[]>(`/api/projects/${projectId}/branches`).then(setBranches).catch(() => {});
  }, [projectId]);

  useEffect(() => {
    setLoading(true);
    const typeParam = typeFilter === 'all' ? '' : typeFilter;
    api.get<UiPreviewArtifact[]>(`/api/projects/${projectId}/ui-previews${qs({ branch, type: typeParam })}`)
      .then(setArtifacts)
      .catch(() => setArtifacts([]))
      .finally(() => setLoading(false));
  }, [projectId, branch, typeFilter]);

  useEffect(() => {
    if (!compareEnabled || !compareBranch) { setCompareData(null); return; }
    api.get<UiPreviewsCompareResponse>(`/api/projects/${projectId}/ui-previews/compare${qs({ base: branch, head: compareBranch })}`)
      .then(setCompareData)
      .catch(() => setCompareData(null));
  }, [projectId, branch, compareBranch, compareEnabled]);

  const toggleCollapsed = (key: string) => {
    setCollapsed(prev => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  };

  const toggleMetaFilter = (key: string, value: string) => {
    if (metaFilter && metaFilter.key === key && metaFilter.value === value) {
      setMetaFilter(null);
    } else {
      setMetaFilter({ key, value });
    }
  };

  const imageUrl = (pipelineId: string, fileId: string) =>
    `/api/projects/${projectId}/pipelines/${pipelineId}/artifacts/${fileId}/view`;

  // Find item config for a file by matching its relative_path against all config items
  const findItemForFile = (artifact: UiPreviewArtifact, file: UiPreviewFile): UiPreviewItem | null => {
    if (!artifact.config) return null;
    const search = (groups: Record<string, UiPreviewGroup>): UiPreviewItem | null => {
      for (const g of Object.values(groups)) {
        if (g.items) {
          for (const [filename, item] of Object.entries(g.items)) {
            if (file.relative_path.endsWith(filename)) return item;
          }
        }
        if (g.groups) {
          const found = search(g.groups);
          if (found) return found;
        }
      }
      return null;
    };
    return search(artifact.config.groups);
  };

  // Check if a file passes the active meta filter
  const passesMetaFilter = (artifact: UiPreviewArtifact, file: UiPreviewFile): boolean => {
    if (!metaFilter) return true;
    const item = findItemForFile(artifact, file);
    if (!item || !item.meta) return false;
    return item.meta[metaFilter.key] === metaFilter.value;
  };

  // Collect all unique meta key-value pairs across all artifacts
  const allMeta = new Map<string, Set<string>>();
  for (const a of artifacts) {
    if (!a.config) continue;
    const collectMeta = (groups: Record<string, UiPreviewGroup>) => {
      for (const g of Object.values(groups)) {
        if (g.items) {
          for (const item of Object.values(g.items)) {
            if (item.meta) {
              for (const [k, v] of Object.entries(item.meta)) {
                if (!allMeta.has(k)) allMeta.set(k, new Set());
                allMeta.get(k)!.add(v);
              }
            }
          }
        }
        if (g.groups) collectMeta(g.groups);
      }
    };
    collectMeta(a.config.groups);
  }

  // Render a group tree recursively
  const renderGroup = (key: string, group: UiPreviewGroup, artifact: UiPreviewArtifact, path: string, depth: number) => {
    const fullKey = path ? `${path}.${key}` : key;
    const isCollapsed = collapsed.has(fullKey);

    // Collect renderable items for this group
    const items: { file: UiPreviewFile; item: UiPreviewItem | null; filename: string }[] = [];
    if (group.items) {
      for (const [filename, item] of Object.entries(group.items)) {
        const file = artifact.files.find(f => f.relative_path.endsWith(filename));
        if (file && passesMetaFilter(artifact, file)) {
          items.push({ file, item, filename });
        }
      }
    }

    const hasSubGroups = group.groups && Object.keys(group.groups).length > 0;
    const hasItems = items.length > 0;
    if (!hasSubGroups && !hasItems && group.items && Object.keys(group.items).length > 0) return null;

    return (
      <div key={fullKey} class="ui-preview-group">
        <div class="ui-preview-group-header" onClick={() => toggleCollapsed(fullKey)}>
          <span class="toggle-icon">{isCollapsed ? '\u25B8' : '\u25BE'}</span>
          {group.label}
        </div>
        {!isCollapsed && (
          <div class="ui-preview-group-children">
            {group.groups && Object.entries(group.groups).map(([k, g]) =>
              renderGroup(k, g, artifact, fullKey, depth + 1)
            )}
            {hasItems && (
              <div class="ui-preview-grid">
                {items.map(({ file, item }) => (
                  <div key={file.id} class="ui-preview-card" onClick={() => setLightbox({ file, artifact, item })}>
                    <img src={imageUrl(artifact.pipeline_id, file.id)}
                      alt={item?.label || file.relative_path}
                      loading="lazy" />
                    <div class="ui-preview-card-label">
                      {item?.label || file.relative_path.split('/').pop()}
                    </div>
                    {item?.meta && (
                      <div style="padding:0 0.5rem 0.4rem">
                        {Object.entries(item.meta).map(([mk, mv]) => (
                          <span key={`${mk}-${mv}`}
                            class={`ui-preview-meta-badge${metaFilter && metaFilter.key === mk && metaFilter.value === mv ? ' active' : ''}`}
                            onClick={(e) => { e.stopPropagation(); toggleMetaFilter(mk, mv); }}>
                            {mv}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    );
  };

  // Render uncategorized files (not referenced in any config item)
  const renderUncategorized = (artifact: UiPreviewArtifact) => {
    if (!artifact.config) {
      // No config at all - show all files flat
      const filtered = artifact.files.filter(f => passesMetaFilter(artifact, f));
      if (filtered.length === 0) return null;
      return (
        <div class="ui-preview-grid">
          {filtered.map(file => (
            <div key={file.id} class="ui-preview-card" onClick={() => setLightbox({ file, artifact, item: null })}>
              <img src={imageUrl(artifact.pipeline_id, file.id)}
                alt={file.relative_path} loading="lazy" />
              <div class="ui-preview-card-label">{file.relative_path.split('/').pop()}</div>
            </div>
          ))}
        </div>
      );
    }

    // Find files not referenced in config
    const referencedFiles = new Set<string>();
    const collectRefs = (groups: Record<string, UiPreviewGroup>) => {
      for (const g of Object.values(groups)) {
        if (g.items) {
          for (const filename of Object.keys(g.items)) {
            for (const f of artifact.files) {
              if (f.relative_path.endsWith(filename)) referencedFiles.add(f.id);
            }
          }
        }
        if (g.groups) collectRefs(g.groups);
      }
    };
    collectRefs(artifact.config.groups);

    const uncategorized = artifact.files.filter(f => !referencedFiles.has(f.id) && passesMetaFilter(artifact, f));
    if (uncategorized.length === 0) return null;

    const isCollapsed = collapsed.has('__uncategorized');
    return (
      <div class="ui-preview-group">
        <div class="ui-preview-group-header" onClick={() => toggleCollapsed('__uncategorized')}>
          <span class="toggle-icon">{isCollapsed ? '\u25B8' : '\u25BE'}</span>
          Uncategorized
        </div>
        {!isCollapsed && (
          <div class="ui-preview-group-children">
            <div class="ui-preview-grid">
              {uncategorized.map(file => (
                <div key={file.id} class="ui-preview-card" onClick={() => setLightbox({ file, artifact, item: null })}>
                  <img src={imageUrl(artifact.pipeline_id, file.id)}
                    alt={file.relative_path} loading="lazy" />
                  <div class="ui-preview-card-label">{file.relative_path.split('/').pop()}</div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    );
  };

  // Compare view: render side by side matched by relative_path
  const renderCompare = () => {
    if (!compareData) return <div class="empty-state">Loading comparison...</div>;
    const baseFiles = new Map<string, { file: UiPreviewFile; artifact: UiPreviewArtifact }>();
    const headFiles = new Map<string, { file: UiPreviewFile; artifact: UiPreviewArtifact }>();

    for (const a of compareData.base) {
      for (const f of a.files) baseFiles.set(f.relative_path, { file: f, artifact: a });
    }
    for (const a of compareData.head) {
      for (const f of a.files) headFiles.set(f.relative_path, { file: f, artifact: a });
    }

    const allPaths = new Set([...baseFiles.keys(), ...headFiles.keys()]);
    if (allPaths.size === 0) return <div class="empty-state">No files to compare</div>;

    return (
      <div class="ui-preview-compare">
        <div class="ui-preview-compare-col">
          <h4>Base: {branch}</h4>
          {[...allPaths].sort().map(path => {
            const entry = baseFiles.get(path);
            return (
              <div key={path} style="margin-bottom:0.75rem">
                <div class="text-xs text-muted mb-sm">{path.split('/').pop()}</div>
                {entry ? (
                  <img src={imageUrl(entry.artifact.pipeline_id, entry.file.id)}
                    style="width:100%;border-radius:var(--radius);border:1px solid var(--border)"
                    loading="lazy" alt={path} />
                ) : (
                  <div class="empty-state" style="padding:2rem;font-size:0.75rem">Not in base</div>
                )}
              </div>
            );
          })}
        </div>
        <div class="ui-preview-compare-col">
          <h4>Head: {compareBranch}</h4>
          {[...allPaths].sort().map(path => {
            const entry = headFiles.get(path);
            return (
              <div key={path} style="margin-bottom:0.75rem">
                <div class="text-xs text-muted mb-sm">{path.split('/').pop()}</div>
                {entry ? (
                  <img src={imageUrl(entry.artifact.pipeline_id, entry.file.id)}
                    style="width:100%;border-radius:var(--radius);border:1px solid var(--border)"
                    loading="lazy" alt={path} />
                ) : (
                  <div class="empty-state" style="padding:2rem;font-size:0.75rem">Not in head</div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    );
  };

  if (loading) return <div class="empty-state">Loading...</div>;

  return (
    <div>
      {/* Controls: branch selector, type filter, compare toggle */}
      <div class="flex gap-sm mb-md" style="align-items:center;flex-wrap:wrap">
        <select class="input" style="width:auto" value={branch}
          onChange={(e) => setBranch((e.target as HTMLSelectElement).value)}>
          {branches.map(b => <option key={b.name} value={b.name}>{b.name}</option>)}
        </select>

        <div class="flex gap-sm">
          {(['all', 'ui-comp', 'ui-flow'] as const).map(t => (
            <button key={t} class={`btn btn-sm${typeFilter === t ? ' btn-primary' : ''}`}
              onClick={() => setTypeFilter(t)}>
              {t === 'all' ? 'All' : t === 'ui-comp' ? 'Components' : 'Flows'}
            </button>
          ))}
        </div>

        <label style="margin-left:auto;display:flex;align-items:center;gap:0.4rem;font-size:0.8rem;color:var(--text-secondary);cursor:pointer">
          <input type="checkbox" checked={compareEnabled}
            onChange={() => { setCompareEnabled(!compareEnabled); if (compareEnabled) { setCompareBranch(''); setCompareData(null); } }} />
          Compare
        </label>

        {compareEnabled && (
          <select class="input" style="width:auto" value={compareBranch}
            onChange={(e) => setCompareBranch((e.target as HTMLSelectElement).value)}>
            <option value="">Select branch...</option>
            {branches.filter(b => b.name !== branch).map(b => (
              <option key={b.name} value={b.name}>{b.name}</option>
            ))}
          </select>
        )}
      </div>

      {/* Active meta filters */}
      {allMeta.size > 0 && (
        <div class="flex gap-sm mb-md" style="flex-wrap:wrap;align-items:center">
          <span class="text-xs text-muted">Filter:</span>
          {[...allMeta.entries()].map(([key, values]) =>
            [...values].sort().map(v => (
              <span key={`${key}-${v}`}
                class={`ui-preview-meta-badge${metaFilter && metaFilter.key === key && metaFilter.value === v ? ' active' : ''}`}
                onClick={() => toggleMetaFilter(key, v)}>
                {key}: {v}
              </span>
            ))
          )}
          {metaFilter && (
            <button class="btn btn-sm" onClick={() => setMetaFilter(null)}>Clear</button>
          )}
        </div>
      )}

      {/* Compare mode */}
      {compareEnabled && compareBranch ? (
        <div class="card">{renderCompare()}</div>
      ) : (
        /* Normal view */
        <div class="card">
          {artifacts.length === 0 ? (
            <div class="empty-state">
              <p>No UI previews yet.</p>
              <p class="text-muted text-sm mt-sm">
                Add a ui-preview step with artifacts to your .platform.yaml to get started.
              </p>
            </div>
          ) : (
            artifacts.map(artifact => (
              <div key={artifact.id} style="margin-bottom:1.5rem">
                <div class="flex-between mb-sm">
                  <h3 style="font-size:0.9rem">{artifact.name}</h3>
                  <Badge status={artifact.artifact_type === 'ui-comp' ? 'component' : 'flow'}>
                    {artifact.artifact_type === 'ui-comp' ? 'Component' : 'Flow'}
                  </Badge>
                </div>
                {artifact.config ? (
                  <>
                    {Object.entries(artifact.config.groups).map(([k, g]) =>
                      renderGroup(k, g, artifact, '', 0)
                    )}
                    {renderUncategorized(artifact)}
                  </>
                ) : (
                  renderUncategorized(artifact)
                )}
              </div>
            ))
          )}
        </div>
      )}

      {/* Lightbox modal */}
      <Modal open={!!lightbox} onClose={() => setLightbox(null)}
        title={lightbox?.item?.label || lightbox?.file.relative_path.split('/').pop() || 'Preview'} wide>
        {lightbox && (
          <div>
            <img class="ui-preview-lightbox-img"
              src={imageUrl(lightbox.artifact.pipeline_id, lightbox.file.id)}
              alt={lightbox.item?.label || lightbox.file.relative_path} />
            <div class="ui-preview-lightbox-meta">
              {lightbox.item?.meta && Object.entries(lightbox.item.meta).map(([k, v]) => (
                <span key={`${k}-${v}`} class="ui-preview-meta-badge">{k}: {v}</span>
              ))}
            </div>
            <div class="text-xs text-muted" style="text-align:center;margin-top:0.5rem">
              {lightbox.file.relative_path}
              {lightbox.file.size_bytes != null && ` (${Math.round(lightbox.file.size_bytes / 1024)} KB)`}
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}

function DeploymentsTab({ projectId }: { projectId: string }) {
  const [deployments, setDeployments] = useState<Deployment[]>([]);
  const [previews, setPreviews] = useState<PreviewDeployment[]>([]);
  const [selectedEnv, setSelectedEnv] = useState<string | null>(null);
  const [showRollback, setShowRollback] = useState(false);
  const [rollbackImage, setRollbackImage] = useState('');

  const load = () => {
    api.get<ListResponse<Deployment>>(`/api/projects/${projectId}/deployments?limit=50`)
      .then(r => setDeployments(r.items)).catch(e => console.warn(e));
    api.get<ListResponse<PreviewDeployment>>(`/api/projects/${projectId}/previews?limit=50`)
      .then(r => setPreviews(r.items)).catch(() => setPreviews([]));
  };

  useEffect(() => {
    load();
    const interval = setInterval(load, 10000);
    return () => clearInterval(interval);
  }, [projectId]);

  // Group deployments by environment
  const envMap = new Map<string, Deployment[]>();
  for (const d of deployments) {
    const list = envMap.get(d.environment) || [];
    list.push(d);
    envMap.set(d.environment, list);
  }

  const envNames = [...envMap.keys()].sort((a, b) => {
    const order: Record<string, number> = { production: 0, staging: 1, preview: 2 };
    return (order[a] ?? 99) - (order[b] ?? 99);
  });

  const rollback = async () => {
    if (!selectedEnv || !rollbackImage) return;
    try {
      await api.patch(`/api/projects/${projectId}/deployments/${selectedEnv}`, {
        image_ref: rollbackImage,
      });
      setShowRollback(false);
      setRollbackImage('');
      load();
    } catch { /* ignore */ }
  };

  const deletePreview = async (slug: string) => {
    if (!confirm('Delete this preview environment?')) return;
    await api.del(`/api/projects/${projectId}/previews/${slug}`);
    load();
  };

  const timeRemaining = (expiresAt: string): string => {
    const ms = new Date(expiresAt).getTime() - Date.now();
    if (ms <= 0) return 'Expired';
    const hours = Math.floor(ms / 3600000);
    if (hours > 0) return `${hours}h left`;
    const mins = Math.floor(ms / 60000);
    return `${mins}m left`;
  };

  return (
    <div>
      {/* Environment cards */}
      <div class="env-cards mb-md">
        {envNames.map(env => {
          const deps = envMap.get(env) || [];
          const latest = deps[0];
          return (
            <div key={env} class={`env-card ${selectedEnv === env ? 'env-card-selected' : ''}`}
              onClick={() => setSelectedEnv(selectedEnv === env ? null : env)}>
              <div class="env-card-name">{env}</div>
              {latest && (
                <div>
                  <StatusDot status={latest.current_status} label={latest.current_status} />
                  <div class="mono text-xs mt-sm truncate">{latest.image_ref}</div>
                  <div class="text-muted text-xs mt-sm">
                    {latest.deployed_at ? timeAgo(latest.deployed_at) : '--'}
                  </div>
                </div>
              )}
              {env !== 'preview' && latest && (
                <button class="btn btn-sm mt-sm" onClick={(e) => {
                  e.stopPropagation();
                  setSelectedEnv(env);
                  setShowRollback(true);
                }}>Rollback</button>
              )}
            </div>
          );
        })}
        {envNames.length === 0 && <div class="empty-state" style="width:100%">No deployments</div>}
      </div>

      {/* Deployment history for selected environment */}
      {selectedEnv && (
        <div class="card mb-md">
          <div class="card-header">
            <span class="card-title">Deployment History ({selectedEnv})</span>
          </div>
          <table class="table">
            <thead><tr><th>Time</th><th>Image</th><th>Desired</th><th>Current</th><th>Deployed By</th></tr></thead>
            <tbody>
              {(envMap.get(selectedEnv) || []).map(d => (
                <tr key={d.id}>
                  <td class="text-muted text-sm">{d.deployed_at ? timeAgo(d.deployed_at) : '--'}</td>
                  <td class="mono text-xs truncate" style="max-width:200px">{d.image_ref}</td>
                  <td><Badge status={d.desired_status} /></td>
                  <td><Badge status={d.current_status} /></td>
                  <td class="text-sm">{d.deployed_by || '--'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Preview environments */}
      {previews.length > 0 && (
        <div class="card">
          <div class="card-header">
            <span class="card-title">Active Previews</span>
          </div>
          <table class="table">
            <thead>
              <tr>
                <th>Branch</th>
                <th>Status</th>
                <th>Image</th>
                <th>Expires</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {previews.map(p => (
                <tr key={p.id}>
                  <td class="mono text-xs">{p.branch}</td>
                  <td><StatusDot status={p.current_status} label={p.current_status} /></td>
                  <td class="mono text-xs truncate" style="max-width:150px">{p.image_ref}</td>
                  <td class="text-sm">{timeRemaining(p.expires_at)}</td>
                  <td>
                    <button class="btn btn-danger btn-sm" onClick={() => deletePreview(p.branch_slug)}>Delete</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <Modal open={showRollback} onClose={() => setShowRollback(false)} title={`Rollback ${selectedEnv}`}>
        <div class="form-group">
          <label>Image to deploy</label>
          <input class="input" value={rollbackImage}
            placeholder="Enter image reference..."
            onInput={(e) => setRollbackImage((e.target as HTMLInputElement).value)} />
        </div>
        <div class="text-sm text-muted mb-md">
          This will deploy the specified image to the {selectedEnv} environment.
        </div>
        <div class="modal-actions">
          <button class="btn" onClick={() => setShowRollback(false)}>Cancel</button>
          <button class="btn btn-primary" onClick={rollback} disabled={!rollbackImage}>Deploy</button>
        </div>
      </Modal>
    </div>
  );
}

interface ResolvedCommand {
  name: string;
  prompt_template: string;
  scope: string;
  persistent_session: boolean;
}

function SkillsTab({ projectId }: { projectId: string }) {
  const [resolved, setResolved] = useState<ResolvedCommand[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ name: '', description: '', prompt_template: '', persistent_session: false });
  const [error, setError] = useState('');

  const load = () => {
    api.get<ResolvedCommand[]>(`/api/commands/resolved${qs({ project_id: projectId })}`)
      .then(setResolved).catch(e => console.warn(e));
  };
  useEffect(load, [projectId]);

  const create = async (e: Event) => {
    e.preventDefault();
    setError('');
    try {
      await api.post('/api/commands', { ...form, project_id: projectId });
      setShowCreate(false);
      setForm({ name: '', description: '', prompt_template: '', persistent_session: false });
      load();
    } catch (err: any) { setError(err.message); }
  };

  return (
    <div>
      <div class="flex-between mb-md">
        <span class="text-muted text-sm">
          Showing resolved skills (project overrides workspace overrides global). Repo commands (.claude/commands/) take highest priority at runtime.
        </span>
        <button class="btn btn-primary btn-sm" onClick={() => { setShowCreate(true); setError(''); }}>New Project Skill</button>
      </div>
      <div class="card">
        {resolved.length === 0 ? <div class="empty-state">No skills defined</div> : (
          <table class="table">
            <thead><tr><th>Name</th><th>Scope</th><th>Persistent</th></tr></thead>
            <tbody>
              {resolved.map(c => (
                <tr key={c.name}>
                  <td class="mono">/{c.name}</td>
                  <td><Badge status={c.scope} /></td>
                  <td>{c.persistent_session ? 'Yes' : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="New Project Skill">
        <form onSubmit={create}>
          <div class="form-group">
            <label>Name</label>
            <input class="input" required placeholder="e.g. dev, review" value={form.name}
              onInput={(e) => setForm({ ...form, name: (e.target as HTMLInputElement).value })} />
          </div>
          <div class="form-group">
            <label>Description</label>
            <input class="input" value={form.description}
              onInput={(e) => setForm({ ...form, description: (e.target as HTMLInputElement).value })} />
          </div>
          <div class="form-group">
            <label>Prompt Template</label>
            <textarea class="input mono" rows={8} required value={form.prompt_template}
              placeholder="Use $ARGUMENTS for user input"
              onInput={(e) => setForm({ ...form, prompt_template: (e.target as HTMLTextAreaElement).value })} />
          </div>
          <div class="form-group">
            <label>
              <input type="checkbox" checked={form.persistent_session}
                onChange={() => setForm({ ...form, persistent_session: !form.persistent_session })} />
              {' '}Persistent session
            </label>
          </div>
          {error && <div class="error-msg">{error}</div>}
          <div class="modal-actions">
            <button type="button" class="btn" onClick={() => setShowCreate(false)}>Cancel</button>
            <button type="submit" class="btn btn-primary">Create</button>
          </div>
        </form>
      </Modal>
    </div>
  );
}

function WebhooksTab({ projectId }: { projectId: string }) {
  const [webhooks, setWebhooks] = useState<Webhook[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ url: '', events: ['push'], secret: '' });
  const [error, setError] = useState('');

  const load = () => {
    api.get<ListResponse<Webhook>>(`/api/projects/${projectId}/webhooks?limit=50`)
      .then(r => setWebhooks(r.items)).catch(e => console.warn(e));
  };
  useEffect(load, [projectId]);

  const create = async (e: Event) => {
    e.preventDefault();
    try {
      await api.post(`/api/projects/${projectId}/webhooks`, {
        url: form.url,
        events: form.events,
        secret: form.secret || undefined,
      });
      setShowCreate(false);
      setForm({ url: '', events: ['push'], secret: '' });
      load();
    } catch (err: any) { setError(err.message); }
  };

  const remove = async (whId: string) => {
    await api.del(`/api/projects/${projectId}/webhooks/${whId}`);
    load();
  };

  return (
    <div>
      <div class="flex-between mb-md">
        <span />
        <button class="btn btn-primary btn-sm" onClick={() => setShowCreate(true)}>New Webhook</button>
      </div>
      <div class="card">
        {webhooks.length === 0 ? <div class="empty-state">No webhooks</div> : (
          <table class="table">
            <thead><tr><th>URL</th><th>Events</th><th>Active</th><th></th></tr></thead>
            <tbody>
              {webhooks.map(w => (
                <tr key={w.id}>
                  <td class="mono text-xs truncate" style="max-width:250px">{w.url}</td>
                  <td class="text-xs">{w.events.join(', ')}</td>
                  <td><Badge status={w.active ? 'active' : 'inactive'} /></td>
                  <td><button class="btn btn-danger btn-sm" onClick={() => remove(w.id)}>Delete</button></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="New Webhook">
        <form onSubmit={create}>
          <div class="form-group">
            <label>URL</label>
            <input class="input" type="url" required value={form.url}
              onInput={(e) => setForm({ ...form, url: (e.target as HTMLInputElement).value })} />
          </div>
          <div class="form-group">
            <label>Events (comma-separated)</label>
            <input class="input" value={form.events.join(',')}
              onInput={(e) => setForm({ ...form, events: (e.target as HTMLInputElement).value.split(',').map(s => s.trim()).filter(Boolean) })} />
          </div>
          <div class="form-group">
            <label>Secret (optional)</label>
            <input class="input" type="password" value={form.secret}
              onInput={(e) => setForm({ ...form, secret: (e.target as HTMLInputElement).value })} />
          </div>
          {error && <div class="error-msg">{error}</div>}
          <div class="modal-actions">
            <button type="button" class="btn" onClick={() => setShowCreate(false)}>Cancel</button>
            <button type="submit" class="btn btn-primary">Create</button>
          </div>
        </form>
      </Modal>
    </div>
  );
}

function SettingsTab({ project, onUpdate }: { project: Project; onUpdate: (p: Project) => void }) {
  const [form, setForm] = useState({
    display_name: project.display_name || '',
    description: project.description || '',
    visibility: project.visibility,
    default_branch: project.default_branch,
  });
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState('');

  const save = async (e: Event) => {
    e.preventDefault();
    setError('');
    try {
      const updated = await api.patch<Project>(`/api/projects/${project.id}`, form);
      onUpdate(updated);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (err: any) { setError(err.message); }
  };

  return (
    <div>
      <div class="card mb-md">
        <div class="card-title mb-md">Project Settings</div>
        <form onSubmit={save}>
          <div class="form-group">
            <label>Display Name</label>
            <input class="input" value={form.display_name}
              onInput={(e) => setForm({ ...form, display_name: (e.target as HTMLInputElement).value })} />
          </div>
          <div class="form-group">
            <label>Description</label>
            <textarea class="input" value={form.description}
              onInput={(e) => setForm({ ...form, description: (e.target as HTMLTextAreaElement).value })} />
          </div>
          <div class="form-group">
            <label>Visibility</label>
            <select class="input" value={form.visibility}
              onChange={(e) => setForm({ ...form, visibility: (e.target as HTMLSelectElement).value })}>
              <option value="private">Private</option>
              <option value="internal">Internal</option>
              <option value="public">Public</option>
            </select>
          </div>
          <div class="form-group">
            <label>Default Branch</label>
            <input class="input" value={form.default_branch}
              onInput={(e) => setForm({ ...form, default_branch: (e.target as HTMLInputElement).value })} />
          </div>
          {error && <div class="error-msg">{error}</div>}
          {saved && <div style="color:var(--success);font-size:13px">Saved</div>}
          <button type="submit" class="btn btn-primary mt-sm">Save Settings</button>
        </form>
      </div>
      {(project.namespace_slug || project.agent_image) && (
        <div class="card mb-md">
          <div class="card-title mb-md">Agent Settings</div>
          <div class="session-meta-list">
            <div class="session-meta-row">
              <span class="text-muted text-sm">Namespace</span>
              <span class="mono text-sm">{project.namespace_slug}</span>
            </div>
            {project.agent_image && (
              <div class="session-meta-row">
                <span class="text-muted text-sm">Agent Image</span>
                <span class="mono text-sm" style="word-break:break-all">{project.agent_image}</span>
              </div>
            )}
          </div>
        </div>
      )}
      <SecretsSection projectId={project.id} />
    </div>
  );
}

function SecretsSection({ projectId }: { projectId: string }) {
  const [secrets, setSecrets] = useState<Secret[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState({ name: '', value: '', scope: 'build' });
  const [error, setError] = useState('');

  const load = () => {
    api.get<ListResponse<Secret>>(`/api/projects/${projectId}/secrets?limit=100`)
      .then(r => setSecrets(r.items)).catch(() => setSecrets([]));
  };
  useEffect(load, [projectId]);

  const create = async (e: Event) => {
    e.preventDefault();
    setError('');
    try {
      await api.post(`/api/projects/${projectId}/secrets`, {
        name: form.name,
        value: form.value,
        scope: form.scope,
      });
      setShowCreate(false);
      setForm({ name: '', value: '', scope: 'build' });
      load();
    } catch (err: any) { setError(err.message); }
  };

  const deleteSecret = async (secretId: string, name: string) => {
    if (!confirm(`Delete secret "${name}"? This action cannot be undone.`)) return;
    await api.del(`/api/projects/${projectId}/secrets/${encodeURIComponent(name)}`);
    load();
  };

  return (
    <div class="card">
      <div class="card-header">
        <span class="card-title">Secrets</span>
        <button class="btn btn-primary btn-sm" onClick={() => setShowCreate(true)}>Add Secret</button>
      </div>
      <div class="text-sm text-muted mb-md">
        Secret values are encrypted and cannot be displayed after creation.
      </div>
      {secrets.length === 0 ? (
        <div class="empty-state">No secrets configured</div>
      ) : (
        <table class="table">
          <thead><tr><th>Name</th><th>Scope</th><th>Version</th><th>Updated</th><th></th></tr></thead>
          <tbody>
            {secrets.map(s => (
              <tr key={s.id}>
                <td class="mono text-sm">{s.name}</td>
                <td class="text-sm"><Badge status={s.scope} /></td>
                <td class="text-sm text-muted">v{s.version}</td>
                <td class="text-muted text-sm">{timeAgo(s.updated_at)}</td>
                <td>
                  <button class="btn btn-danger btn-sm" onClick={() => deleteSecret(s.id, s.name)}>Delete</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="Add Secret">
        <form onSubmit={create}>
          <div class="form-group">
            <label>Name</label>
            <input class="input" required value={form.name}
              placeholder="SECRET_NAME"
              onInput={(e) => setForm({ ...form, name: (e.target as HTMLInputElement).value })} />
          </div>
          <div class="form-group">
            <label>Value</label>
            <textarea class="input" required value={form.value}
              rows={3}
              onInput={(e) => setForm({ ...form, value: (e.target as HTMLTextAreaElement).value })} />
            <div class="text-xs mt-sm" style="color:var(--warning)">
              This value will not be shown again after creation.
            </div>
          </div>
          <div class="form-group">
            <label>Scope</label>
            <select class="input" value={form.scope}
              onChange={(e) => setForm({ ...form, scope: (e.target as HTMLSelectElement).value })}>
              <option value="build">Build</option>
              <option value="deploy">Deploy</option>
              <option value="all">All</option>
            </select>
          </div>
          {error && <div class="error-msg">{error}</div>}
          <div class="modal-actions">
            <button type="button" class="btn" onClick={() => setShowCreate(false)}>Cancel</button>
            <button type="submit" class="btn btn-primary">Create</button>
          </div>
        </form>
      </Modal>
    </div>
  );
}
