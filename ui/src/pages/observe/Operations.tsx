import { useState, useEffect } from 'preact/hooks';
import { api } from '../../lib/api';
import { ObserveTab } from '../../components/ObserveTab';

interface Project { id: string; name: string; display_name: string | null; }
interface ListResponse<T> { items: T[]; total: number; }

export function Operations() {
  const [projectId, setProjectId] = useState<string | undefined>(undefined);
  const [projects, setProjects] = useState<Project[]>([]);

  useEffect(() => {
    api.get<ListResponse<Project>>('/api/projects?limit=100')
      .then(r => setProjects(r.items)).catch(() => {});
  }, []);

  return (
    <div>
      <div class="page-header" style="display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem">
        <h2 style="margin:0">Operations</h2>
        <select class="input" style="width:auto;min-width:200px"
          value={projectId || ''}
          onChange={e => {
            const v = (e.target as HTMLSelectElement).value;
            setProjectId(v || undefined);
          }}>
          <option value="">Platform Infrastructure</option>
          {projects.map(p => (
            <option key={p.id} value={p.id}>{p.display_name || p.name}</option>
          ))}
        </select>
      </div>
      <ObserveTab projectId={projectId} platformMode={!projectId} />
    </div>
  );
}
