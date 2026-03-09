import { useState, useEffect } from 'preact/hooks';
import { api } from '../lib/api';
import { timeAgo } from '../lib/format';

interface ProviderKeyMeta {
  provider: string;
  key_suffix: string;
  created_at: string;
  updated_at: string;
}

interface CredentialStatus {
  exists: boolean;
  auth_type?: string;
  token_expires_at?: string;
  created_at?: string;
  updated_at?: string;
}

export function ProviderKeys() {
  const [keys, setKeys] = useState<ProviderKeyMeta[]>([]);
  const [apiKey, setApiKey] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');

  // CLI OAuth state
  const [cliCreds, setCliCreds] = useState<CredentialStatus>({ exists: false });
  const [oauthToken, setOauthToken] = useState('');
  const [oauthSaving, setOauthSaving] = useState(false);
  const [oauthError, setOauthError] = useState('');
  const [oauthSuccess, setOauthSuccess] = useState('');

  const load = () => {
    api.get<ProviderKeyMeta[]>('/api/users/me/provider-keys')
      .then(setKeys).catch(() => {});
  };

  const loadCliCreds = () => {
    api.get<CredentialStatus>('/api/auth/cli-credentials')
      .then(setCliCreds).catch(() => {});
  };

  useEffect(() => { load(); loadCliCreds(); }, []);

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

  const saveOauth = async (e: Event) => {
    e.preventDefault();
    setOauthError('');
    setOauthSuccess('');
    setOauthSaving(true);
    try {
      await api.post('/api/auth/cli-credentials', { auth_type: 'oauth', token: oauthToken });
      setOauthToken('');
      setOauthSuccess('OAuth token saved');
      loadCliCreds();
    } catch (err: any) {
      setOauthError(err.message);
    } finally {
      setOauthSaving(false);
    }
  };

  const removeOauth = async () => {
    setOauthError('');
    setOauthSuccess('');
    try {
      await api.del('/api/auth/cli-credentials');
      setOauthSuccess('OAuth token removed');
      loadCliCreds();
    } catch (err: any) {
      setOauthError(err.message);
    }
  };

  return (
    <div>
      <h2 style="margin-bottom:1rem">Provider Keys</h2>
      <p class="text-muted text-sm mb-md">
        Provide your own API key or OAuth token for agent sessions. If not set, the platform's shared key is used.
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

      <div class="card" style="margin-top:1rem">
        <div class="card-header">
          <span class="card-title">Claude CLI OAuth Token</span>
        </div>
        <div style="padding:1rem">
          {cliCreds.exists ? (
            <div class="flex-between mb-md">
              <div>
                <span class="badge">{cliCreds.auth_type}</span>
                <span class="text-muted text-sm" style="margin-left:0.5rem">
                  Updated {timeAgo(cliCreds.updated_at!)}
                </span>
              </div>
              <button class="btn btn-danger btn-sm" onClick={removeOauth}>Remove</button>
            </div>
          ) : (
            <div class="text-muted text-sm mb-md">No token configured</div>
          )}

          {cliCreds.exists && existing && (
            <div class="text-sm mb-md" style="color:var(--color-warning, #e8a317)">
              OAuth token is active and takes priority over the API key for agent sessions.
            </div>
          )}

          <form onSubmit={saveOauth}>
            <div class="form-group">
              <label>{cliCreds.exists ? 'Replace token' : 'Set token'}</label>
              <input
                class="input"
                type="password"
                placeholder="Paste OAuth token..."
                value={oauthToken}
                onInput={(e) => setOauthToken((e.target as HTMLInputElement).value)}
                minLength={10}
              />
            </div>
            <p class="text-muted text-sm" style="margin-bottom:0.5rem">
              OAuth tokens take priority over API keys for agent sessions.
            </p>
            {oauthError && <div class="error-msg">{oauthError}</div>}
            {oauthSuccess && <div class="success-msg">{oauthSuccess}</div>}
            <button type="submit" class="btn btn-primary btn-sm" disabled={oauthSaving || oauthToken.length < 10}>
              {oauthSaving ? 'Saving...' : 'Save Token'}
            </button>
          </form>
        </div>
      </div>
    </div>
  );
}
