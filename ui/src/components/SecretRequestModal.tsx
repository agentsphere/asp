import { useState } from 'preact/hooks';
import { api } from '../lib/api';
import { Modal } from './Modal';

interface Props {
  open: boolean;
  projectId: string;
  requestId: string;
  name: string;
  prompt: string;
  onComplete: () => void;
  onClose: () => void;
}

export function SecretRequestModal({ open, projectId, requestId, name, prompt, onComplete, onClose }: Props) {
  const [value, setValue] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!value.trim() || submitting) return;
    setSubmitting(true);
    setError('');
    try {
      await api.post(`/api/projects/${projectId}/secret-requests/${requestId}`, {
        value: value.trim(),
      });
      setValue('');
      onComplete();
    } catch (err: any) {
      setError(err.message || 'Failed to submit secret');
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal open={open} onClose={onClose} title="Secret Requested">
      <div class="mb-md">
        <div class="text-sm text-muted mb-sm">The agent is requesting the following secret:</div>
        <div class="mono text-sm" style="background:var(--bg-secondary);padding:0.5rem;border-radius:4px">
          {name}
        </div>
      </div>
      {prompt && (
        <div class="mb-md">
          <div class="text-sm text-muted mb-sm">Reason:</div>
          <div class="text-sm">{prompt}</div>
        </div>
      )}
      <form onSubmit={submit}>
        <div class="form-group">
          <label>Secret Value</label>
          <input
            class="input"
            type="password"
            required
            value={value}
            placeholder="Enter the secret value..."
            onInput={(e) => setValue((e.target as HTMLInputElement).value)}
            disabled={submitting}
          />
          <div class="text-xs mt-sm" style="color:var(--warning)">
            This value will be encrypted and stored securely.
          </div>
        </div>
        {error && <div class="error-msg">{error}</div>}
        <div class="modal-actions">
          <button type="button" class="btn" onClick={onClose} disabled={submitting}>
            Dismiss
          </button>
          <button type="submit" class="btn btn-primary" disabled={submitting || !value.trim()}>
            {submitting ? 'Submitting...' : 'Provide Secret'}
          </button>
        </div>
      </form>
    </Modal>
  );
}
