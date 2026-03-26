import { createContext } from 'preact';
import { useContext, useState, useEffect, useCallback } from 'preact/hooks';
import { api } from './api';

interface OnboardingStatus {
  has_projects: boolean;
  has_provider_key: boolean;
  has_cli_credentials: boolean;
  needs_onboarding: boolean;
}

interface OnboardingState {
  needsOnboarding: boolean;
  hasProviderKey: boolean;
  hasCliCredentials: boolean;
  loading: boolean;
  refresh: () => void;
}

const OnboardingContext = createContext<OnboardingState>({
  needsOnboarding: false,
  hasProviderKey: false,
  hasCliCredentials: false,
  loading: true,
  refresh: () => {},
});

export function OnboardingProvider({ children }: { children: any }) {
  const [status, setStatus] = useState<OnboardingStatus | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(() => {
    setLoading(true);
    api.get<OnboardingStatus>('/api/onboarding/status')
      .then(s => setStatus(s))
      // Onboarding status check failed — skip
      .catch(() => setStatus(null))
      .finally(() => setLoading(false));
  }, []);

  useEffect(refresh, [refresh]);

  return (
    <OnboardingContext.Provider value={{
      needsOnboarding: status?.needs_onboarding ?? false,
      hasProviderKey: status?.has_provider_key ?? false,
      hasCliCredentials: status?.has_cli_credentials ?? false,
      loading,
      refresh,
    }}>
      {children}
    </OnboardingContext.Provider>
  );
}

export function useOnboarding(): OnboardingState {
  return useContext(OnboardingContext);
}
