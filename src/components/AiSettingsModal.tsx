import { type FormEvent, useEffect, useRef, useState } from 'react';
import { Loader } from '@redrob-labs/ui';
import { Icon } from '../ui/Icon';
import { useWorkspace } from '../store/WorkspaceProvider';
import { type AiKeyStatus, REDROB_LAUNCHER_RELEASES_URL } from '../api/bridge';

type Feedback = { tone: 'success' | 'error'; message: string } | null;

/** The engine's own sentence for its absence ("Redrob Code is not installed..." or "The Redrob Code
 * engine could not be started: ..."), when that is what a bridge call rejected with. */
const engineSentence = (error: unknown): string | null =>
  typeof error === 'string' && (error.startsWith('Redrob Code is not installed') || error.startsWith('The Redrob Code engine could not be started')) ? error : null;

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
  const [keyStatus, setKeyStatus] = useState<AiKeyStatus | null>(null);
  // The engine's own sentence when Redrob Code is not installed or would not start.
  const [engineError, setEngineError] = useState<string | null>(null);
  const [statusReads, setStatusReads] = useState(0);

  // Read fresh each time the dialog opens (and after a save): the person may sign in or out in
  // Redrob Code or Redrob Launcher while this app is open, and the dialog must say what the engine
  // would use for the next AI request.
  useEffect(() => {
    if (!open || !bridge.aiKeyStatus) return;
    let cancelled = false;
    setKeyStatus(null);
    setEngineError(null);
    bridge.aiKeyStatus().then(
      (status) => { if (!cancelled) setKeyStatus(status); },
      (error: unknown) => { if (!cancelled) setEngineError(engineSentence(error) ?? 'The Redrob Code engine could not be reached.'); },
    );
    return () => { cancelled = true; };
  }, [open, bridge, statusReads]);

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
          : 'Redrob key handed to the Redrob Code engine, which keeps it.',
      });
      setStatusReads((count) => count + 1);
    } catch (error: unknown) {
      setKey('');
      // The engine names its own absence ("Redrob Code is not installed...") and nothing else is
      // shown verbatim: the key is never part of a message.
      setFeedback({ tone: 'error', message: engineSentence(error) ?? 'Could not hand the Redrob key to the Redrob Code engine. Try again.' });
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
          <div><span className="modal-icon"><Icon name="key" size={24} /></span><div><h2 id="ai-settings-title">Redrob settings</h2><p>Connect Redrob AI securely</p></div></div>
          <button type="button" aria-label="Close Redrob settings" data-testid="close-ai-settings" onClick={close} disabled={saving}><Icon name="close" size={24} /></button>
        </div>
        <form id="ai-settings-form" className="ai-settings-form" onSubmit={save}>
          <div className="ai-settings-security"><Icon name="shieldCheck" size={24} /><div><strong>Secure by design</strong><p id="ai-settings-description">In the desktop app, your Redrob key is handed to the shared Redrob Code engine on this computer, which keeps it and calls Redrob with it. This app stores no key, and the key is never added to queries or AI messages.</p></div></div>
          {engineError ? (
            <div className="ai-settings-feedback error" data-testid="ai-engine-missing" role="alert">
              <Icon name="close" />
              <span>
                {engineError.endsWith(REDROB_LAUNCHER_RELEASES_URL)
                  ? <>{engineError.slice(0, -REDROB_LAUNCHER_RELEASES_URL.length)}<a href={REDROB_LAUNCHER_RELEASES_URL} target="_blank" rel="noreferrer">Get Redrob Launcher</a></>
                  : engineError}
              </span>
            </div>
          ) : null}
          {keyStatus?.source === 'launcher' ? (
            <div className="ai-settings-security ai-settings-launcher" data-testid="ai-key-launcher" role="status">
              <Icon name="check" size={24} />
              <div>
                <strong>Signed in with Redrob Launcher</strong>
                <p>The engine uses Redrob Launcher's sign-in. Sign out in Redrob Launcher. A key saved here is handed to the engine and used instead.</p>
              </div>
            </div>
          ) : null}
          {keyStatus?.source === 'engine' ? (
            <div className="ai-settings-security ai-settings-launcher" data-testid="ai-key-engine" role="status">
              <Icon name="check" size={24} />
              <div>
                <strong>Signed in to Redrob</strong>
                <p>The Redrob Code engine holds a Redrob key. Saving a key here replaces it.</p>
              </div>
            </div>
          ) : null}
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
          <small id="ai-key-hint">Paste a key beginning with <code>rrk_</code>. Saving hands it to the Redrob Code engine and replaces the key the engine holds.</small>
          <div className="demo-credential-notice"><strong>{bridge.mode === 'demo' ? 'Browser demo' : 'AI request context'}</strong><span>{bridge.mode === 'demo' ? 'This demo does not send credentials or store your key. AI responses are generated locally.' : 'Desktop AI sends your prompt and optional active query to Redrob through the Redrob Code engine. Database result rows and credentials are not automatically attached. The desktop renderer trusts user-entered content in those fields, so anything you include there is sent.'}</span></div>
          {feedback ? <div id="ai-key-feedback" className={`ai-settings-feedback ${feedback.tone}`} role={feedback.tone === 'error' ? 'alert' : 'status'} data-testid="ai-settings-feedback">{feedback.tone === 'success' ? <Icon name="check" /> : <Icon name="close" />}{feedback.message}</div> : <span id="ai-key-feedback" />}
        </form>
        <div className="modal-footer ai-settings-footer"><span><i /> {bridge.mode === 'demo' ? 'Demo mode · no credential network request' : 'Desktop mode · key kept by the Redrob Code engine'}</span><div><button type="button" className="ghost-button" onClick={close} disabled={saving}>Cancel</button><button type="submit" form="ai-settings-form" className="primary-button" data-testid="save-ai-key" disabled={saving}>{saving ? <Loader size="sm" label="Saving the key" /> : null}{saving ? 'Saving…' : 'Save key'}</button></div></div>
      </section>
    </div>
  );
}
