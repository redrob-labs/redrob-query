import { type FormEvent, useEffect, useRef, useState } from 'react';
import { Check, KeyRound, ShieldCheck, X } from 'lucide-react';
import { useWorkspace } from '../store/WorkspaceProvider';

type Feedback = { tone: 'success' | 'error'; message: string } | null;

const validateKey = (value: string): string | null => {
  if (!value) return 'Enter your Redrob API key.';
  if (!value.startsWith('rrk_')) return 'Redrob API keys start with rrk_.';
  if (!/^rrk_[A-Za-z0-9_-]{8,}$/.test(value)) return 'Enter a valid Redrob API key with no spaces.';
  return null;
};

export function AiSettingsModal() {
  const open = useWorkspace((state) => state.aiSettingsOpen);
  const bridge = useWorkspace((state) => state.bridge);
  const setUi = useWorkspace((state) => state.setUi);
  const inputRef = useRef<HTMLInputElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const wasOpenRef = useRef(false);
  const [key, setKey] = useState('');
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(null);

  useEffect(() => {
    if (open && !wasOpenRef.current) {
      openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      setKey('');
      setSaving(false);
      setFeedback(null);
      requestAnimationFrame(() => inputRef.current?.focus());
    } else if (!open && wasOpenRef.current) {
      setKey('');
      setSaving(false);
      setFeedback(null);
      openerRef.current?.focus();
    }
    wasOpenRef.current = open;
  }, [open]);

  if (!open) return null;

  const close = () => {
    setKey('');
    setFeedback(null);
    setUi({ aiSettingsOpen: false });
  };

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (saving) return;
    const secret = key.trim();
    const validationError = validateKey(secret);
    if (validationError) {
      setFeedback({ tone: 'error', message: validationError });
      return;
    }

    setSaving(true);
    setFeedback(null);
    try {
      await bridge.saveAiKey(secret);
      setKey('');
      setFeedback({
        tone: 'success',
        message: bridge.mode === 'demo'
          ? 'Demo acknowledged. Browser demo does not send or store credentials.'
          : 'Redrob key saved securely in your OS keychain.',
      });
    } catch {
      setKey('');
      setFeedback({ tone: 'error', message: 'Could not save the Redrob key. Check secure storage access and try again.' });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !saving) close(); }}>
      <section
        className="ai-settings-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="ai-settings-title"
        aria-describedby="ai-settings-description"
        data-testid="ai-settings-modal"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            if (!saving) close();
          }
          if (event.key !== 'Tab') return;
          const controls = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), [tabindex="0"]'));
          const first = controls[0];
          const last = controls.at(-1);
          if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
          if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
        }}
      >
        <div className="modal-heading">
          <div><span className="modal-icon"><KeyRound size={18} /></span><div><h2 id="ai-settings-title">Redrob settings</h2><p>Connect Redrob AI securely</p></div></div>
          <button type="button" aria-label="Close Redrob settings" data-testid="close-ai-settings" onClick={close} disabled={saving}><X size={18} /></button>
        </div>
        <form id="ai-settings-form" className="ai-settings-form" onSubmit={save}>
          <div className="ai-settings-security"><ShieldCheck size={20} /><div><strong>Secure by design</strong><p id="ai-settings-description">In the desktop app, your Redrob key is stored in the operating system keychain. It is never added to queries or AI messages.</p></div></div>
          <label htmlFor="redrob-api-key">Redrob API key</label>
          <input
            ref={inputRef}
            id="redrob-api-key"
            data-testid="ai-key-input"
            type="password"
            value={key}
            onChange={(event) => { setKey(event.target.value); setFeedback(null); }}
            placeholder="rrk_••••••••••••"
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            aria-invalid={feedback?.tone === 'error'}
            aria-describedby="ai-key-hint ai-key-feedback"
            disabled={saving}
          />
          <small id="ai-key-hint">Paste a key beginning with <code>rrk_</code>. Saving replaces the key currently stored by the desktop app.</small>
          <div className="demo-credential-notice"><strong>{bridge.mode === 'demo' ? 'Browser demo' : 'AI request context'}</strong><span>{bridge.mode === 'demo' ? 'This demo does not send credentials or store your key. AI responses are generated locally.' : 'Desktop AI sends your prompt and optional active query to Redrob. Database result rows and credentials are not automatically attached. The desktop renderer trusts user-entered content in those fields, so anything you include there is sent.'}</span></div>
          {feedback ? <div id="ai-key-feedback" className={`ai-settings-feedback ${feedback.tone}`} role={feedback.tone === 'error' ? 'alert' : 'status'} data-testid="ai-settings-feedback">{feedback.tone === 'success' ? <Check size={15} /> : <X size={15} />}{feedback.message}</div> : <span id="ai-key-feedback" />}
        </form>
        <div className="modal-footer ai-settings-footer"><span><i /> {bridge.mode === 'demo' ? 'Demo mode · no credential network request' : 'Desktop mode · OS keychain storage'}</span><div><button type="button" className="ghost-button" onClick={close} disabled={saving}>Cancel</button><button type="submit" form="ai-settings-form" className="primary-button" data-testid="save-ai-key" disabled={saving}>{saving ? <span className="spinner small" /> : null}{saving ? 'Saving…' : 'Save key'}</button></div></div>
      </section>
    </div>
  );
}
