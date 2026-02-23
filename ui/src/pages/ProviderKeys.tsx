import { useState, useEffect } from 'preact/hooks';
import { api } from '../lib/api';
import { timeAgo } from '../lib/format';

interface ProviderKeyMeta {
  provider: string;
  key_suffix: string;
  created_at: string;
  updated_at: string;
}

export function ProviderKeys() {
  const [keys, setKeys] = useState<ProviderKeyMeta[]>([]);
  const [apiKey, setApiKey] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');

  const load = () => {
    api.get<ProviderKeyMeta[]>('/api/users/me/provider-keys')
      .then(setKeys).catch(() => {});
  };
  useEffect(load, []);

  const existing = keys.find(k => k.provider === 'anthropic');

  const save = async (e: Event) => {
    e.preventDefault();
    setError('');
    setSuccess('');
    setSaving(true);
    try {
      await api.put('/api/users/me/provider-keys/anthropic', { api_key: apiKey });
      setApiKey('');
      setSuccess('API key saved');
      load();
    } catch (err: any) {
      setError(err.message);
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    setError('');
    setSuccess('');
    try {
      await api.del('/api/users/me/provider-keys/anthropic');
      setSuccess('API key removed');
      load();
    } catch (err: any) {
      setError(err.message);
    }
  };

  return (
    <div>
      <h2 style="margin-bottom:1rem">Provider Keys</h2>
      <p class="text-muted text-sm mb-md">
        Provide your own API key for agent sessions. If not set, the platform's shared key is used.
      </p>

      <div class="card">
        <div class="card-header">
          <span class="card-title">Anthropic API Key</span>
        </div>
        <div style="padding:1rem">
          {existing ? (
            <div class="flex-between mb-md">
              <div>
                <span class="mono text-sm">{existing.key_suffix}</span>
                <span class="text-muted text-sm" style="margin-left:0.5rem">
                  Updated {timeAgo(existing.updated_at)}
                </span>
              </div>
              <button class="btn btn-danger btn-sm" onClick={remove}>Remove</button>
            </div>
          ) : (
            <div class="text-muted text-sm mb-md">No key configured</div>
          )}

          <form onSubmit={save}>
            <div class="form-group">
              <label>{existing ? 'Replace key' : 'Set key'}</label>
              <input
                class="input"
                type="password"
                placeholder="sk-ant-api03-..."
                value={apiKey}
                onInput={(e) => setApiKey((e.target as HTMLInputElement).value)}
                minLength={10}
              />
            </div>
            {error && <div class="error-msg">{error}</div>}
            {success && <div class="success-msg">{success}</div>}
            <button type="submit" class="btn btn-primary btn-sm" disabled={saving || apiKey.length < 10}>
              {saving ? 'Saving...' : 'Save Key'}
            </button>
          </form>
        </div>
      </div>
    </div>
  );
}
