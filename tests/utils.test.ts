import { describe, expect, it } from 'vitest';
import { validateProfileInput, errorMessage } from '../src/lib/utils';

const valid = { name: 'Work', baseUrl: 'https://gateway.example.test/custom/v2', model: 'provider-model', apiKey: 'fake-key' };
describe('connection input safety', () => {
  it('accepts a provider-specific path without assuming /v1', () => expect(validateProfileInput(valid)).toBeNull());
  it.each(['http://gateway.example.test', 'file:///tmp/key', 'https://user:pass@example.test', 'https://example.test?token=secret', 'https://example.test#token'])('rejects unsafe or wrong endpoint %s', (baseUrl) => expect(validateProfileInput({ ...valid, baseUrl })).not.toBeNull());
  it('allows resource suffixes for native discovery to normalize', () => expect(validateProfileInput({ ...valid, baseUrl: 'https://example.test/v1/responses' })).toBeNull());
  it('allows a channel before models are discovered', () => expect(validateProfileInput({ ...valid, model: '', models: [] })).toBeNull());
  it.each(['http://localhost:8080/v1', 'http://127.0.0.1:8080/api', 'http://[::1]:8080/v1'])('accepts loopback development endpoint %s', (baseUrl) => expect(validateProfileInput({ ...valid, baseUrl })).toBeNull());
  it('requires a new key, but permits retaining a saved key on edit', () => {
    const input = { ...valid, apiKey: undefined };
    expect(validateProfileInput(input)).not.toBeNull();
    expect(validateProfileInput(input, true)).toBeNull();
  });
  it('removes common credential formats from surfaced errors', () => {
    const redacted = errorMessage('sk-exampleSecret123 Bearer secret-value');
    expect(redacted).not.toContain('exampleSecret123');
    expect(redacted).not.toContain('secret-value');
  });
});
