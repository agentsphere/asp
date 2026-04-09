import { useState, useEffect, useRef, useCallback } from 'preact/hooks';
import { api, qs } from '../lib/api';
import { Overlay } from './Overlay';

// ---- Types (matching backend response shapes) ----

interface ComponentHealth {
  name: string;
  ready: boolean;
  replicas: number;
  ready_replicas: number;
  restarts: number;
  oom_kills: number;
  cpu_used_millicores: number;
  cpu_request: number;
  cpu_limit: number;
  mem_used_bytes: number;
  mem_request: number;
  mem_limit: number;
  avg_rps: number;
  cpu_history: number[];
  mem_history: number[];
  rps_history: number[];
}

interface TopologyEdge { from_service: string; to_service: string; call_count: number; error_count: number; p50_ms: number; }
interface TopologyResponse { edges: TopologyEdge[]; services: string[]; }

interface LoadPoint { ts: string; rps: number; errors: number; }
interface DeployMarker { ts: string; image: string; env: string; }
interface LoadResponse { points: LoadPoint[]; deploys: DeployMarker[]; }

interface ErrorGroup { error_type: string; endpoint: string; count: number; last_seen: string; }

interface TraceAggRow { name: string; count: number; avg_duration_ms: number; error_rate: number; p99_duration_ms: number; }

interface TraceSummary { trace_id: string; root_span: string; service: string; status: string; duration_ms: number; started_at: string; }
interface ListResponse<T> { items: T[]; total: number; }

interface AlertRule { id: string; name: string; severity: string; enabled: boolean; }

interface LogEntry { timestamp: string; service: string; level: string; message: string; trace_id?: string; span_id?: string; }

interface LogTemplate { template: string; count: number; level: string; sample: string; }

interface MetricDataPoint { timestamp: string; value: number; }
interface MetricSeriesResp { name: string; labels: Record<string, string>; points: MetricDataPoint[]; }

// ---- Helpers ----

function fmtTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit' });
}

function formatBytes(b: number): string {
  if (b >= 1073741824) return (b / 1073741824).toFixed(1) + ' GiB';
  if (b >= 1048576) return (b / 1048576).toFixed(0) + ' MiB';
  if (b >= 1024) return (b / 1024).toFixed(0) + ' KiB';
  return b + ' B';
}

export function extractTemplates(logs: LogEntry[]): LogTemplate[] {
  const templateOf = (msg: string) =>
    msg.replace(/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi, '{id}')
       .replace(/\b\d+\.\d+\.\d+\.\d+\b/g, '{ip}')
       .replace(/\b\d+(\.\d+)?\b/g, '{n}')
       .replace(/\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g, '{email}');
  const groups = new Map<string, { count: number; level: string; sample: string }>();
  for (const log of logs) {
    const tmpl = templateOf(log.message);
    const existing = groups.get(tmpl);
    if (existing) existing.count++;
    else groups.set(tmpl, { count: 1, level: log.level, sample: log.message });
  }
  return Array.from(groups, ([template, v]) => ({ template, ...v }))
    .sort((a, b) => b.count - a.count);
}

// ---- Sub-components ----

function Sparkline({ data, height = 20, color = 'var(--accent)' }: { data: number[]; height?: number; color?: string }) {
  if (data.length < 2) return null;
  const max = Math.max(...data, 1);
  const w = 60;
  const pts = data.map((v, i) => `${(i / (data.length - 1)) * w},${height - (v / max) * (height - 2)}`).join(' ');
  return <svg viewBox={`0 0 ${w} ${height}`} width={w} height={height} style="display:block"><polyline points={pts} fill="none" stroke={color} stroke-width="1.5" /></svg>;
}

function CommGraph({ edges, services, components }: { edges: TopologyEdge[]; services: string[]; components: ComponentHealth[] }) {
  // Build positions: spread services in a circle or grid
  const positions: Record<string, { x: number; y: number }> = {};
  const known: Record<string, { x: number; y: number }> = {
    web: { x: 200, y: 50 }, worker: { x: 400, y: 50 },
    db: { x: 120, y: 170 }, cache: { x: 320, y: 170 },
    platform: { x: 260, y: 110 }, postgres: { x: 120, y: 170 },
    valkey: { x: 400, y: 170 }, minio: { x: 260, y: 200 },
  };
  const allNames = [...new Set([...services, ...components.map(c => c.name)])];
  allNames.forEach((name, i) => {
    const lo = name.toLowerCase();
    if (known[lo]) positions[name] = known[lo];
    else positions[name] = { x: 80 + (i % 4) * 130, y: 50 + Math.floor(i / 4) * 120 };
  });

  const compMap = new Map(components.map(c => [c.name, c]));

  return (
    <svg viewBox="0 0 520 230" class="obs-comm-svg">
      {edges.map((e, i) => {
        const from = positions[e.from_service];
        const to = positions[e.to_service];
        if (!from || !to) return null;
        const hasErrors = e.error_count > 0;
        const color = hasErrors ? 'var(--danger)' : 'var(--success)';
        const mx = (from.x + to.x) / 2;
        const my = (from.y + to.y) / 2 - 6;
        return (
          <g key={i}>
            <line x1={from.x} y1={from.y} x2={to.x} y2={to.y}
              stroke={color} stroke-width={Math.min(Math.max(e.call_count / 100, 1.5), 5)}
              opacity={hasErrors ? 0.9 : 0.5} stroke-linecap="round" />
            <text x={mx} y={my} text-anchor="middle" fill="var(--text-muted)" font-size="9">
              {e.call_count}{e.error_count > 0 ? ` (${e.error_count} err)` : ''} · {e.p50_ms.toFixed(0)}ms
            </text>
          </g>
        );
      })}
      {allNames.map(name => {
        const pos = positions[name];
        if (!pos) return null;
        const c = compMap.get(name);
        const healthy = c ? c.ready : true;
        return (
          <g key={name}>
            <circle cx={pos.x} cy={pos.y} r="28"
              fill={healthy ? 'rgba(34,197,94,0.12)' : 'rgba(239,68,68,0.12)'}
              stroke={healthy ? 'var(--success)' : 'var(--danger)'} stroke-width="1.5" />
            <text x={pos.x} y={pos.y + 1} text-anchor="middle" dominant-baseline="middle"
              fill="var(--text-primary)" font-size="12" font-weight="500">{name}</text>
            {c && (
              <text x={pos.x} y={pos.y + 40} text-anchor="middle" fill="var(--text-muted)" font-size="9">
                {c.ready_replicas}/{c.replicas} · {c.avg_rps.toFixed(1)} rps
              </text>
            )}
          </g>
        );
      })}
    </svg>
  );
}

function LoadTimeline({ points, deploys }: { points: LoadPoint[]; deploys: DeployMarker[] }) {
  if (points.length < 2) return <div style="color:var(--muted);padding:1rem">No request load data</div>;

  const W = 900;
  const H = 140;
  const PAD_BOTTOM = 22;
  const chartH = H - PAD_BOTTOM;

  const mapped = points.map(p => ({ ts: new Date(p.ts).getTime(), rps: p.rps, errors: p.errors }));
  const minTs = mapped[0].ts;
  const maxTs = mapped[mapped.length - 1].ts;
  const tsRange = maxTs - minTs || 1;
  const maxRps = Math.max(...mapped.map(p => p.rps), 1);

  const xForTs = (ts: number) => ((ts - minTs) / tsRange) * W;
  const yForRps = (rps: number) => chartH - (rps / maxRps) * (chartH - 10);

  const rpsPath = mapped.map((p, i) => `${i === 0 ? 'M' : 'L'}${xForTs(p.ts)},${yForRps(p.rps)}`).join(' ');
  const areaPath = rpsPath + ` L${xForTs(mapped[mapped.length - 1].ts)},${chartH} L${xForTs(mapped[0].ts)},${chartH} Z`;
  const errorDots = mapped.filter(p => p.errors > 0).map(p => ({ x: xForTs(p.ts), y: yForRps(p.rps) }));

  const mappedDeploys = deploys.map(d => ({ ts: new Date(d.ts).getTime(), image: d.image }));
  const visibleDeploys = mappedDeploys.filter(d => d.ts >= minTs && d.ts <= maxTs);

  const labelCount = 6;
  const labelStep = tsRange / labelCount;
  const timeLabels = Array.from({ length: labelCount + 1 }, (_, i) => minTs + i * labelStep);

  return (
    <svg viewBox={`0 0 ${W} ${H}`} class="obs-timeline-svg" preserveAspectRatio="none">
      <defs>
        <linearGradient id="tlGrad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--accent)" stop-opacity="0.25" />
          <stop offset="100%" stop-color="var(--accent)" stop-opacity="0.02" />
        </linearGradient>
      </defs>
      <path d={areaPath} fill="url(#tlGrad)" />
      <path d={rpsPath} fill="none" stroke="var(--accent)" stroke-width="1.5" />
      {errorDots.map((d, i) => <circle key={i} cx={d.x} cy={d.y} r="3.5" fill="var(--danger)" opacity="0.85" />)}
      {visibleDeploys.map((d, i) => {
        const x = xForTs(d.ts);
        return (
          <g key={i}>
            <line x1={x} y1={0} x2={x} y2={chartH} stroke="var(--accent)" stroke-width="1" stroke-dasharray="4,3" opacity="0.6" />
            <text x={x + 3} y={10} fill="var(--accent)" font-size="8" opacity="0.8">{d.image}</text>
          </g>
        );
      })}
      <text x="4" y="12" fill="var(--text-muted)" font-size="9">{maxRps.toFixed(0)} rps</text>
      {timeLabels.map((ts, i) => (
        <text key={i} x={xForTs(ts)} y={H - 4} fill="var(--text-muted)" font-size="9" text-anchor="middle">{fmtTime(ts)}</text>
      ))}
      <line x1={0} y1={chartH} x2={W} y2={chartH} stroke="var(--border)" stroke-width="0.5" />
    </svg>
  );
}

function MetricChart({ data, unit, height = 160 }: { data: MetricSeriesResp[]; unit: string; height?: number }) {
  if (data.length === 0) return null;
  const colors = ['var(--accent)', '#a855f7', '#22c55e', '#f59e0b', '#ef4444'];

  const series = data.map((s, idx) => ({
    service: s.labels?.service || s.name,
    color: colors[idx % colors.length],
    points: s.points.map(p => ({ ts: new Date(p.timestamp).getTime(), value: p.value })),
  }));

  const allPts = series.flatMap(s => s.points);
  if (allPts.length < 2) return null;

  const W = 800;
  const H = height;
  const PAD = 22;
  const chartH = H - PAD;
  const maxVal = Math.max(...allPts.map(p => p.value), 1);
  const minTs = Math.min(...allPts.map(p => p.ts));
  const maxTs = Math.max(...allPts.map(p => p.ts));
  const tsRange = maxTs - minTs || 1;

  const x = (ts: number) => ((ts - minTs) / tsRange) * W;
  const y = (v: number) => chartH - (v / maxVal) * (chartH - 8);

  const labelCount = 5;
  const labelStep = tsRange / labelCount;
  const labels = Array.from({ length: labelCount + 1 }, (_, i) => minTs + i * labelStep);

  return (
    <div>
      <svg viewBox={`0 0 ${W} ${H}`} class="obs-metric-svg" preserveAspectRatio="none">
        {[0.25, 0.5, 0.75].map(frac => (
          <line key={frac} x1={0} y1={chartH * (1 - frac)} x2={W} y2={chartH * (1 - frac)}
            stroke="var(--border)" stroke-width="0.5" opacity="0.4" />
        ))}
        {series.map(s => {
          const path = s.points.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.ts)},${y(p.value)}`).join(' ');
          return <path key={s.service} d={path} fill="none" stroke={s.color} stroke-width="1.5" opacity="0.85" />;
        })}
        <text x="4" y="12" fill="var(--text-muted)" font-size="9">{maxVal.toFixed(0)} {unit}</text>
        {labels.map((ts, i) => (
          <text key={i} x={x(ts)} y={H - 4} fill="var(--text-muted)" font-size="9" text-anchor="middle">{fmtTime(ts)}</text>
        ))}
        <line x1={0} y1={chartH} x2={W} y2={chartH} stroke="var(--border)" stroke-width="0.5" />
      </svg>
      <div class="obs-metric-legend">
        {series.map(s => (
          <span key={s.service} class="obs-metric-legend-item">
            <span class="obs-metric-legend-dot" style={`background:${s.color}`} />
            {s.service}
          </span>
        ))}
      </div>
    </div>
  );
}

// ---- Trace waterfall helpers ----

interface WaterfallSpan {
  span_id: string;
  name: string;
  service: string;
  status: string;
  duration_ms: number;
  offset_ms: number;
  depth: number;
}

/** Compute offset_ms (relative to trace start) and depth from raw spans. */
function computeWaterfallSpans(spans: any[]): WaterfallSpan[] {
  if (spans.length === 0) return [];

  // Find the earliest started_at as the trace baseline
  const times = spans.map(s => new Date(s.started_at).getTime());
  const traceStart = Math.min(...times);

  // Build parent->children map for depth
  const depthMap = new Map<string, number>();
  const parentMap = new Map<string, string | null>();
  for (const s of spans) {
    parentMap.set(s.span_id, s.parent_span_id || null);
  }

  function getDepth(spanId: string): number {
    if (depthMap.has(spanId)) return depthMap.get(spanId)!;
    const parentId = parentMap.get(spanId);
    const d = parentId && parentMap.has(parentId) ? getDepth(parentId) + 1 : 0;
    depthMap.set(spanId, d);
    return d;
  }

  return spans.map(s => ({
    span_id: s.span_id,
    name: s.name,
    service: s.service,
    status: s.status,
    duration_ms: s.duration_ms || 0,
    offset_ms: new Date(s.started_at).getTime() - traceStart,
    depth: getDepth(s.span_id),
  }));
}

// ---- Main ObserveTab (live data) ----

interface ObserveTabProps {
  projectId?: string;
  platformMode?: boolean;
}

export function ObserveTab({ projectId, platformMode }: ObserveTabProps) {
  const [presetRange, setPresetRange] = useState('1h');
  const [traceTab, setTraceTab] = useState<'recent' | 'aggregated'>('recent');
  const [openTrace, setOpenTrace] = useState<string | null>(null);

  // Data state
  const [components, setComponents] = useState<ComponentHealth[]>([]);
  const [topology, setTopology] = useState<TopologyResponse>({ edges: [], services: [] });
  const [load, setLoad] = useState<LoadResponse>({ points: [], deploys: [] });
  const [errors, setErrors] = useState<ErrorGroup[]>([]);
  const [traceAgg, setTraceAgg] = useState<TraceAggRow[]>([]);
  const [traces, setTraces] = useState<TraceSummary[]>([]);
  const [alerts, setAlerts] = useState<AlertRule[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [logTemplates, setLogTemplates] = useState<LogTemplate[]>([]);
  const [traceDetail, setTraceDetail] = useState<any>(null);
  const [cpuMetrics, setCpuMetrics] = useState<MetricSeriesResp[]>([]);
  const [memMetrics, setMemMetrics] = useState<MetricSeriesResp[]>([]);
  const [rtMetrics, setRtMetrics] = useState<MetricSeriesResp[]>([]);

  // Fetch all data
  useEffect(() => {
    const r = presetRange;
    const p = projectId;
    Promise.all([
      api.get<ComponentHealth[]>(`/api/observe/components${qs({ project_id: p })}`).catch(() => []),
      api.get<TopologyResponse>(`/api/observe/topology${qs({ project_id: p, range: r })}`).catch(() => ({ edges: [], services: [] })),
      api.get<LoadResponse>(`/api/observe/load${qs({ project_id: p, range: r })}`).catch(() => ({ points: [], deploys: [] })),
      api.get<ErrorGroup[]>(`/api/observe/errors${qs({ project_id: p, range: r })}`).catch(() => []),
      api.get<TraceAggRow[]>(`/api/observe/traces/aggregated${qs({ project_id: p, range: r })}`).catch(() => []),
      api.get<ListResponse<TraceSummary>>(`/api/observe/traces${qs({ project_id: p, limit: 20 })}`).catch(() => ({ items: [], total: 0 })),
      api.get<ListResponse<AlertRule>>(`/api/observe/alerts${qs({ project_id: p })}`).catch(() => ({ items: [], total: 0 })),
    ]).then(([comp, topo, ld, errs, agg, tr, al]) => {
      setComponents(comp);
      setTopology(topo);
      setLoad(ld);
      setErrors(errs);
      setTraceAgg(agg);
      setTraces(tr.items);
      setAlerts(al.items);
    });

    // Fetch logs for templates
    api.get<ListResponse<LogEntry>>(`/api/observe/logs${qs({ project_id: p, limit: 500 })}`)
      .then(resp => {
        setLogs(resp.items.slice(0, 20));
        setLogTemplates(extractTemplates(resp.items));
      })
      .catch(() => {});

    // Fetch metric charts
    api.get<MetricSeriesResp[]>(`/api/observe/metrics/query${qs({ name: 'process.cpu.utilization', project_id: p, range: r })}`).then(setCpuMetrics).catch(() => setCpuMetrics([]));
    api.get<MetricSeriesResp[]>(`/api/observe/metrics/query${qs({ name: 'process.memory.rss', project_id: p, range: r })}`).then(setMemMetrics).catch(() => setMemMetrics([]));
    api.get<MetricSeriesResp[]>(`/api/observe/metrics/query${qs({ name: 'http.server.request.duration_sum', project_id: p, range: r })}`).then(setRtMetrics).catch(() => setRtMetrics([]));
  }, [projectId, presetRange]);

  // Auto-refresh components every 30s
  useEffect(() => {
    const id = setInterval(() => {
      api.get<ComponentHealth[]>(`/api/observe/components${qs({ project_id: projectId })}`)
        .then(setComponents).catch(() => {});
    }, 30000);
    return () => clearInterval(id);
  }, [projectId]);

  // Trace detail overlay
  const handleOpenTrace = (traceId: string) => {
    setOpenTrace(traceId);
    api.get<any>(`/api/observe/traces/${traceId}`).then(setTraceDetail).catch(() => {});
  };

  const ranges = ['5m', '15m', '1h', '6h', '24h'];

  return (
    <div class="obs-dashboard">
      {/* Range selector */}
      <div class="obs-section-header" style="margin-bottom: 1rem">
        <span class="obs-section-title">Observability</span>
        <div style="display:flex;gap:4px">
          {ranges.map(r => (
            <button key={r} class={`btn btn-xs ${r === presetRange ? 'btn-primary' : 'btn-ghost'}`} onClick={() => setPresetRange(r)}>{r}</button>
          ))}
        </div>
      </div>

      {/* SLO & Alerts bar */}
      <div class="obs-slo-alert-bar">
        <div class="obs-slos">
          <span class="obs-slo-chip"><span class="obs-slo-dot" style="background:var(--muted)" /> SLO data deferred</span>
        </div>
        <div class="obs-alerts-summary">
          {alerts.length > 0 ? (
            <span class="obs-alert-badge">{alerts.length} alert{alerts.length !== 1 ? 's' : ''}</span>
          ) : (
            <span style="color:var(--muted)">No alerts</span>
          )}
        </div>
      </div>

      {/* Components */}
      <div class="obs-components obs-section">
        <div class="obs-section-header"><span class="obs-section-title">Components</span></div>
        {components.length === 0 ? (
          <div style="color:var(--muted);padding:1rem">No component data available</div>
        ) : (
          <div class="obs-comp-grid">
            {components.map(c => (
              <div key={c.name} class="obs-comp-card">
                <div class="obs-comp-header">
                  <span class="obs-comp-name">{c.name}</span>
                  <span class="obs-comp-meta">{c.ready_replicas}/{c.replicas} ready</span>
                </div>
                <div class="obs-probes">
                  <span class="obs-probe"><span class={`obs-probe-dot`} style={`background:${c.ready ? 'var(--success)' : 'var(--danger)'}`} /><span class="text-xs">R</span></span>
                  {c.restarts > 0 && <span class="obs-probe" title={`${c.restarts} restarts`}><span class="obs-probe-dot" style="background:var(--warning)" /><span class="text-xs">{c.restarts}↻</span></span>}
                  {c.oom_kills > 0 && <span class="obs-probe" title={`${c.oom_kills} OOM kills`}><span class="obs-probe-dot" style="background:var(--danger)" /><span class="text-xs">{c.oom_kills} OOM</span></span>}
                </div>
                <div class="obs-comp-sparks">
                  <div class="obs-spark-item"><span>CPU</span><Sparkline data={c.cpu_history} color="var(--accent)" /></div>
                  <div class="obs-spark-item"><span>MEM</span><Sparkline data={c.mem_history} color="#a855f7" /></div>
                  <div class="obs-spark-item"><span>RPS</span><Sparkline data={c.rps_history} color="#22c55e" /></div>
                </div>
                <div class="obs-resource">
                  <div class="obs-resource-header"><span>CPU</span><span>{c.cpu_used_millicores.toFixed(0)}m / {c.cpu_limit}m</span></div>
                  <div class="obs-resource-bar">
                    <div class="obs-resource-fill" style={`width:${Math.min(100, c.cpu_limit > 0 ? (c.cpu_used_millicores / c.cpu_limit) * 100 : 0)}%`} />
                    {c.cpu_request > 0 && c.cpu_limit > 0 && <div class="obs-resource-request" style={`left:${Math.min(100, (c.cpu_request / c.cpu_limit) * 100)}%`} />}
                  </div>
                </div>
                <div class="obs-resource">
                  <div class="obs-resource-header"><span>MEM</span><span>{formatBytes(c.mem_used_bytes)} / {formatBytes(c.mem_limit)}</span></div>
                  <div class="obs-resource-bar">
                    <div class="obs-resource-fill" style={`width:${Math.min(100, c.mem_limit > 0 ? (c.mem_used_bytes / c.mem_limit) * 100 : 0)}%`} />
                    {c.mem_request > 0 && c.mem_limit > 0 && <div class="obs-resource-request" style={`left:${Math.min(100, (c.mem_request / c.mem_limit) * 100)}%`} />}
                  </div>
                </div>
                <div style="font-size:0.7rem;color:var(--text-muted);margin-top:4px">{c.avg_rps.toFixed(1)} rps · {c.restarts} restarts</div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Two-column split: Communication + Request Load */}
      <div class="obs-split">
        <div class="obs-comm obs-section">
          <div class="obs-section-header"><span class="obs-section-title">Communication</span></div>
          {topology.edges.length === 0 && topology.services.length === 0 ? (
            <div style="color:var(--muted);padding:1rem">No service communication data</div>
          ) : (
            <div class="obs-comm-container">
              <CommGraph edges={topology.edges} services={topology.services} components={components} />
            </div>
          )}
        </div>

        <div class="obs-section">
          <div class="obs-section-header"><span class="obs-section-title">Request Load</span></div>
          <LoadTimeline points={load.points} deploys={load.deploys} />
        </div>
      </div>

      {/* Error breakdown */}
      <div class="obs-section">
        <div class="obs-section-header"><span class="obs-section-title">Error Breakdown</span></div>
        {errors.length === 0 ? (
          <div style="color:var(--muted);padding:1rem">No errors in selected range</div>
        ) : (
          <table class="table" style="font-size:0.85rem">
            <thead><tr><th>Type</th><th>Endpoint</th><th>Count</th><th>Last Seen</th></tr></thead>
            <tbody>
              {errors.map((e, i) => (
                <tr key={i}><td>{e.error_type}</td><td><code>{e.endpoint}</code></td><td>{e.count}</td><td>{new Date(e.last_seen).toLocaleTimeString()}</td></tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* Alerts */}
      {alerts.length > 0 && (
        <div class="obs-section">
          <div class="obs-section-header"><span class="obs-section-title">Alerts</span></div>
          {alerts.map(a => (
            <div key={a.id} class="obs-alert-row">
              <span class={`obs-alert-indicator ${a.enabled ? 'firing' : 'resolved'}`} />
              <div class="obs-alert-info">
                <strong>{a.name}</strong>
                <div class="obs-alert-meta">{a.severity}</div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Traces */}
      <div class="obs-section">
        <div class="obs-section-header">
          <span class="obs-section-title">Traces</span>
          <div style="display:flex;gap:4px">
            <button class={`btn btn-xs ${traceTab === 'recent' ? 'btn-primary' : 'btn-ghost'}`} onClick={() => setTraceTab('recent')}>Recent</button>
            <button class={`btn btn-xs ${traceTab === 'aggregated' ? 'btn-primary' : 'btn-ghost'}`} onClick={() => setTraceTab('aggregated')}>Aggregated</button>
          </div>
        </div>
        {traceTab === 'recent' ? (
          traces.length === 0 ? (
            <div style="color:var(--muted);padding:1rem">No recent traces</div>
          ) : (
            <table class="table" style="font-size:0.85rem">
              <thead><tr><th>Operation</th><th>Service</th><th>Status</th><th>Duration</th><th>Time</th></tr></thead>
              <tbody>
                {traces.map(t => (
                  <tr key={t.trace_id} style="cursor:pointer" onClick={() => handleOpenTrace(t.trace_id)}>
                    <td>{t.root_span}</td><td>{t.service}</td>
                    <td><span class={`obs-trace-status obs-trace-${t.status}`}>{t.status}</span></td>
                    <td>{t.duration_ms}ms</td>
                    <td>{new Date(t.started_at).toLocaleTimeString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )
        ) : (
          traceAgg.length === 0 ? (
            <div style="color:var(--muted);padding:1rem">No aggregated trace data</div>
          ) : (
            <table class="table" style="font-size:0.85rem">
              <thead><tr><th>Operation</th><th>Count</th><th>Avg (ms)</th><th>p99 (ms)</th><th>Error %</th></tr></thead>
              <tbody>
                {traceAgg.map((t, i) => (
                  <tr key={i}><td>{t.name}</td><td>{t.count}</td><td>{t.avg_duration_ms.toFixed(1)}</td><td>{t.p99_duration_ms.toFixed(1)}</td><td style={t.error_rate > 0 ? 'color:var(--danger)' : ''}>{t.error_rate.toFixed(1)}%</td></tr>
                ))}
              </tbody>
            </table>
          )
        )}
      </div>

      {/* Two-column split: Log Templates + System Logs */}
      <div class="obs-split">
        <div class="obs-section">
          <div class="obs-section-header"><span class="obs-section-title">Log Templates</span></div>
          {logTemplates.length === 0 ? (
            <div style="color:var(--muted);padding:1rem">No logs in selected range</div>
          ) : (
            <table class="table" style="font-size:0.8rem">
              <thead><tr><th>Count</th><th>Level</th><th>Template</th></tr></thead>
              <tbody>
                {logTemplates.slice(0, 15).map((t, i) => (
                  <tr key={i}><td>{t.count}</td><td><span class={`obs-log-level obs-log-${t.level}`}>{t.level}</span></td><td style="font-family:var(--font-mono);font-size:0.75rem">{t.template}</td></tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        <div class="obs-section">
          <div class="obs-section-header"><span class="obs-section-title">Recent Logs</span></div>
          {logs.length === 0 ? (
            <div style="color:var(--muted);padding:1rem">No recent logs</div>
          ) : (
            <table class="table" style="font-size:0.8rem">
              <thead><tr><th>Time</th><th>Level</th><th>Service</th><th>Message</th></tr></thead>
              <tbody>
                {logs.map((l, i) => (
                  <tr key={i}><td style="white-space:nowrap">{new Date(l.timestamp).toLocaleTimeString()}</td><td><span class={`obs-log-level obs-log-${l.level}`}>{l.level}</span></td><td>{l.service}</td><td style="font-family:var(--font-mono);font-size:0.75rem;max-width:400px;overflow:hidden;text-overflow:ellipsis">{l.message}</td></tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {/* Metrics charts (3-column grid matching mock layout) */}
      <div class="obs-metrics-grid">
        <div class="obs-section">
          <div class="obs-section-header"><span class="obs-section-title">CPU Utilization</span></div>
          <MetricChart data={cpuMetrics} unit="m" />
        </div>
        <div class="obs-section">
          <div class="obs-section-header"><span class="obs-section-title">Memory Usage</span></div>
          <MetricChart data={memMetrics} unit="MiB" />
        </div>
        <div class="obs-section" style="grid-column:1/-1">
          <div class="obs-section-header"><span class="obs-section-title">Response Time</span></div>
          <MetricChart data={rtMetrics} unit="ms" />
        </div>
      </div>

      {/* Trace detail overlay */}
      {openTrace && (
        <Overlay title={`Trace ${openTrace.slice(0, 8)}...`} onClose={() => { setOpenTrace(null); setTraceDetail(null); }}>
          {traceDetail ? (
            <div>
              <div style="margin-bottom:0.5rem;font-size:0.85rem">
                <strong>{traceDetail.root_span}</strong> &mdash; {traceDetail.service} &mdash;
                <span class={`obs-trace-status obs-trace-${traceDetail.status}`} style="margin:0 0.3rem">{traceDetail.status}</span>
                &mdash; {traceDetail.duration_ms}ms
              </div>
              {traceDetail.spans && traceDetail.spans.length > 0 ? (() => {
                const wSpans = computeWaterfallSpans(traceDetail.spans);
                const maxEnd = Math.max(...wSpans.map(s => s.offset_ms + s.duration_ms), 1);
                return (
                  <div class="obs-waterfall">
                    {wSpans.map(s => {
                      const leftPct = (s.offset_ms / maxEnd) * 100;
                      const widthPct = Math.max((s.duration_ms / maxEnd) * 100, 0.5);
                      const color = s.status === 'error' ? 'var(--danger)' : 'var(--accent)';
                      return (
                        <div key={s.span_id} class="obs-waterfall-row">
                          <div class="obs-waterfall-label" style={`padding-left:${s.depth * 1}rem`}>
                            <span class="text-xs mono">{s.service}</span>
                            <span class="text-xs">{s.name}</span>
                          </div>
                          <div class="obs-waterfall-bar-container">
                            <div class="obs-waterfall-bar" style={`left:${leftPct}%;width:${widthPct}%;background:${color}`} />
                            <span class="obs-waterfall-dur" style={`left:${leftPct + widthPct + 0.5}%`}>{s.duration_ms}ms</span>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                );
              })() : (
                <div style="color:var(--muted)">No spans</div>
              )}
            </div>
          ) : (
            <div style="color:var(--muted);padding:2rem;text-align:center">Loading trace...</div>
          )}
        </Overlay>
      )}
    </div>
  );
}
