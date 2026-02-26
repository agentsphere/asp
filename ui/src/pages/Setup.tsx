import { useState } from 'preact/hooks';
import { api, ApiError } from '../lib/api';

interface SetupResponse {
  id: string;
  name: string;
  email: string;
  message: string;
}

export function Setup() {
  const [token, setToken] = useState('');
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [done, setDone] = useState(false);

  const submit = async (e: Event) => {
    e.preventDefault();
    setError('');

    if (password !== confirm) {
      setError('Passwords do not match');
      return;
    }
    if (password.length < 8) {
      setError('Password must be at least 8 characters');
      return;
    }

    setLoading(true);
    try {
      await api.post<SetupResponse>('/api/setup', {
        token,
        name,
        email,
        password,
        display_name: displayName || undefined,
      });
      setDone(true);
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.status === 401) {
          setError('Invalid or expired setup token');
        } else if (err.status === 404) {
          setError('Setup already completed');
        } else if (err.status === 429) {
          setError('Too many attempts. Please wait before trying again.');
        } else {
          setError(err.body.error || 'Setup failed');
        }
      } else {
        setError('Setup failed');
      }
    } finally {
      setLoading(false);
    }
  };

  if (done) {
    return (
      <div class="login-page">
        <div class="card login-card">
          <h1 class="login-title">Setup Complete</h1>
          <p style="text-align:center;margin-bottom:1rem">
            Admin account created successfully. You can now sign in.
          </p>
          <button
            class="btn btn-primary"
            style="width:100%"
            onClick={() => { window.location.href = '/'; }}
          >
            Go to Login
          </button>
        </div>
      </div>
    );
  }

  return (
    <div class="login-page">
      <div class="card login-card">
        <h1 class="login-title">Platform Setup</h1>
        <p style="text-align:center;margin-bottom:1rem;color:var(--text-secondary)">
          Create the first admin account to get started.
        </p>
        <form onSubmit={submit}>
          <div class="form-group">
            <label>Setup Token</label>
            <input class="input" type="text" value={token}
              onInput={(e) => setToken((e.target as HTMLInputElement).value)}
              placeholder="Paste the token from server logs"
              autoFocus required />
          </div>
          <div class="form-group">
            <label>Username</label>
            <input class="input" type="text" value={name}
              onInput={(e) => setName((e.target as HTMLInputElement).value)}
              required />
          </div>
          <div class="form-group">
            <label>Display Name (optional)</label>
            <input class="input" type="text" value={displayName}
              onInput={(e) => setDisplayName((e.target as HTMLInputElement).value)} />
          </div>
          <div class="form-group">
            <label>Email</label>
            <input class="input" type="email" value={email}
              onInput={(e) => setEmail((e.target as HTMLInputElement).value)}
              required />
          </div>
          <div class="form-group">
            <label>Password</label>
            <input class="input" type="password" value={password}
              onInput={(e) => setPassword((e.target as HTMLInputElement).value)}
              minLength={8} required />
          </div>
          <div class="form-group">
            <label>Confirm Password</label>
            <input class="input" type="password" value={confirm}
              onInput={(e) => setConfirm((e.target as HTMLInputElement).value)}
              minLength={8} required />
          </div>
          {error && <div class="error-msg">{error}</div>}
          <button class="btn btn-primary" style="width:100%;margin-top:0.5rem"
            type="submit" disabled={loading}>
            {loading ? 'Creating account...' : 'Create Admin Account'}
          </button>
        </form>
      </div>
    </div>
  );
}
