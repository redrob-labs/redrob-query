import { ArrowRight, Check, GitCompareArrows, Trash2, X } from 'lucide-react';
import { useWorkspace } from '../store/WorkspaceProvider';

const display = (value: unknown) => value === null ? 'NULL' : String(value);

export function ChangesPanel() {
  const open = useWorkspace((state) => state.changesOpen);
  const mutations = useWorkspace((state) => state.mutations);
  const status = useWorkspace((state) => state.mutationStatus);
  const discard = useWorkspace((state) => state.discardMutation);
  const discardAll = useWorkspace((state) => state.discardAllMutations);
  const apply = useWorkspace((state) => state.applyMutations);
  const setUi = useWorkspace((state) => state.setUi);
  if (!open) return null;
  return (
    <section className="changes-panel" aria-label="Staged changes" data-testid="changes-panel">
      <div className="changes-heading"><span className="changes-icon"><GitCompareArrows size={16} /></span><div><h2>Staged changes</h2><p>{mutations.length ? `Review ${mutations.length} pending cell edit${mutations.length === 1 ? '' : 's'}` : 'Your edited cells will appear here'}</p></div><button className="panel-close" aria-label="Close changes panel" onClick={() => setUi({ changesOpen: false })}><X size={16} /></button></div>
      <div className="changes-list">
        {!mutations.length ? <div className="changes-empty"><Check size={20} /><span>No pending changes</span></div> : mutations.map((mutation) => <div className="change-card" key={mutation.id}>
          <div><strong>{mutation.table}</strong><span>{mutation.rowKey} · {mutation.column}</span></div>
          <div className="value-diff"><del>{display(mutation.previousValue)}</del><ArrowRight size={13} /><ins>{display(mutation.nextValue)}</ins></div>
          <button aria-label={`Discard change to ${mutation.column}`} onClick={() => discard(mutation.id)}><Trash2 size={14} /></button>
        </div>)}
      </div>
      <div className="changes-actions"><button className="ghost-button" onClick={discardAll} disabled={!mutations.length}>Discard all</button><button className="primary-button" data-testid="apply-changes" onClick={() => void apply()} disabled={!mutations.length || status === 'loading'}>{status === 'loading' ? <span className="spinner small" /> : <Check size={14} />} Apply {mutations.length || ''}</button></div>
    </section>
  );
}
