import { Loader } from '@redrob-labs/ui';
import { Icon } from '../ui/Icon';
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
      <div className="changes-heading"><span className="changes-icon"><Icon name="compare" /></span><div><h2>Staged changes</h2><p>{mutations.length ? `Review ${mutations.length} pending cell edit${mutations.length === 1 ? '' : 's'}` : 'Your edited cells will appear here'}</p></div><button className="panel-close" aria-label="Close changes panel" onClick={() => setUi({ changesOpen: false })}><Icon name="close" /></button></div>
      <div className="changes-list">
        {!mutations.length ? <div className="changes-empty"><Icon name="check" size={24} /><span>No pending changes</span></div> : mutations.map((mutation) => <div className="change-card" key={mutation.id}>
          {mutation.kind === 'insert' ? <>
            <div><strong>{mutation.table}</strong><span>New row</span></div>
            <div className="value-diff is-insert"><Icon name="plus" /> {(mutation.values ?? []).map((item) => `${item.column} = ${display(item.value)}`).join(', ')}</div>
            <button aria-label="Discard the new row" onClick={() => discard(mutation.id)}><Icon name="close" /></button>
          </> : mutation.kind === 'delete' ? <>
            <div><strong>{mutation.table}</strong><span>{mutation.primaryKey} {mutation.rowKey}</span></div>
            <div className="value-diff is-delete"><Icon name="trash" /> Delete this row</div>
            <button aria-label={`Keep row ${mutation.rowKey}`} onClick={() => discard(mutation.id)}><Icon name="close" /></button>
          </> : <>
            <div><strong>{mutation.table}</strong><span>{mutation.rowKey} · {mutation.column}</span></div>
            <div className="value-diff"><del>{display(mutation.previousValue)}</del><Icon name="arrowRight" /><ins>{display(mutation.nextValue)}</ins></div>
            <button aria-label={`Discard change to ${mutation.column}`} onClick={() => discard(mutation.id)}><Icon name="trash" /></button>
          </>}
        </div>)}
      </div>
      <div className="changes-actions"><button className="ghost-button" onClick={discardAll} disabled={!mutations.length}>Discard all</button><button className="primary-button" data-testid="apply-changes" onClick={() => void apply()} disabled={!mutations.length || status === 'loading'}>{status === 'loading' ? <Loader size="sm" label="Applying staged changes" /> : <Icon name="check" />} Apply {mutations.length || ''}</button></div>
    </section>
  );
}
