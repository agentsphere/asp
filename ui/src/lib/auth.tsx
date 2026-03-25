import { createContext } from 'preact';
import { useContext, useState, useEffect } from 'preact/hooks';
import { api } from './api';
import { prepareRequestOptions, serializeAuthResponse } from './webauthn';
import type { User, BeginLoginResponse, PasskeyLoginResponse } from './types';

interface AuthState {
  user: User | null;
  loading: boolean;
  login: (name: string, password: string) => Promise<void>;
  loginWithPasskey: () => Promise<void>;
  logout: () => Promise<void>;
  setUser: (user: User) => void;
}

const AuthContext = createContext<AuthState>(null!);

export function AuthProvider({ children }: { children: any }) {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api.get<User>('/api/auth/me')
      .then(u => setUser(u))
      .catch(() => setUser(null))
      .finally(() => setLoading(false));
  }, []);

  const login = async (name: string, password: string) => {
    const res = await api.post<{ user: User }>('/api/auth/login', { name, password });
    setUser(res.user);
  };

  const loginWithPasskey = async () => {
    const beginResp = await api.post<BeginLoginResponse>('/api/auth/passkeys/login/begin', {});
    const opts = prepareRequestOptions(beginResp.challenge);
    const credential = await navigator.credentials.get({ publicKey: opts }) as PublicKeyCredential;
    if (!credential) throw new Error('Passkey authentication was cancelled');
    const serialized = serializeAuthResponse(credential);
    const completeResp = await api.post<PasskeyLoginResponse>('/api/auth/passkeys/login/complete', {
      challenge_id: beginResp.challenge_id,
      credential: serialized,
    });
    setUser(completeResp.user);
  };

  const logout = async () => {
    await api.post('/api/auth/logout').catch(() => {});
    setUser(null);
  };

  return (
    <AuthContext.Provider value={{ user, loading, login, loginWithPasskey, logout, setUser }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthState {
  return useContext(AuthContext);
}
