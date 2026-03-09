// WebAuthn browser API helpers.
// Converts between webauthn-rs JSON format and navigator.credentials API.

export function base64urlToBuffer(s: string): ArrayBuffer {
  // base64url → base64 → binary
  const base64 = s.replace(/-/g, '+').replace(/_/g, '/');
  const pad = base64.length % 4;
  const padded = pad ? base64 + '='.repeat(4 - pad) : base64;
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

export function bufferToBase64url(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  let binary = '';
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function prepareCreationOptions(serverChallenge: any): PublicKeyCredentialCreationOptions {
  const pk = serverChallenge.publicKey || serverChallenge;
  const opts: PublicKeyCredentialCreationOptions = {
    ...pk,
    challenge: base64urlToBuffer(pk.challenge),
    user: {
      ...pk.user,
      id: base64urlToBuffer(pk.user.id),
    },
  };
  if (pk.excludeCredentials) {
    opts.excludeCredentials = pk.excludeCredentials.map((c: any) => ({
      ...c,
      id: base64urlToBuffer(c.id),
    }));
  }
  return opts;
}

export function prepareRequestOptions(serverChallenge: any): PublicKeyCredentialRequestOptions {
  const pk = serverChallenge.publicKey || serverChallenge;
  const opts: PublicKeyCredentialRequestOptions = {
    ...pk,
    challenge: base64urlToBuffer(pk.challenge),
  };
  if (pk.allowCredentials) {
    opts.allowCredentials = pk.allowCredentials.map((c: any) => ({
      ...c,
      id: base64urlToBuffer(c.id),
    }));
  }
  return opts;
}

export function serializeRegistrationResponse(cred: PublicKeyCredential): object {
  const response = cred.response as AuthenticatorAttestationResponse;
  return {
    id: cred.id,
    rawId: bufferToBase64url(cred.rawId),
    type: cred.type,
    response: {
      attestationObject: bufferToBase64url(response.attestationObject),
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
    },
  };
}

export function serializeAuthResponse(cred: PublicKeyCredential): object {
  const response = cred.response as AuthenticatorAssertionResponse;
  return {
    id: cred.id,
    rawId: bufferToBase64url(cred.rawId),
    type: cred.type,
    response: {
      authenticatorData: bufferToBase64url(response.authenticatorData),
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      signature: bufferToBase64url(response.signature),
      userHandle: response.userHandle ? bufferToBase64url(response.userHandle) : null,
    },
  };
}
