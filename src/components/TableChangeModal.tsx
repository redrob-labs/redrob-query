import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { Button, Modal } from '@redrob-labs/ui';
import { Icon } from '../ui/Icon';
import { useWorkspace } from '../store/WorkspaceProvider';
import type { ColumnSpec, ColumnType } from '../domain/types';

// The form behind "New table" and "Add column". Types are chosen from the list the core maps per
// engine; there is no free-text type, because a type goes into the DDL as written.
const TYPES: { value: ColumnType; label: string }[] = [
  { value: 'text', label: 'Text' },
  { value: 'integer', label: 'Whole number' },
  { value: 'decimal', label: 'Decimal' },
  { value: 'boolean', label: 'Yes / no' },
  { value: 'date', label: 'Date' },
  { value: 'timestamp', label: 'Date and time' },
];

const blank = (primaryKey = false): ColumnSpec => ({ name: '', columnType: primaryKey ? 'integer' : 'text', nullable: !primaryKey, primaryKey });

export function TableChangeModal() {
  const target = useWorkspace((state) => state.tableChangeTarget);
  const setUi = useWorkspace((state) => state.setUi);
  const apply = useWorkspace((state) => state.applyTableChange);
  const [schema, setSchema] = useState('');
  const [table, setTable] = useState('');
  const [columns, setColumns] = useState<ColumnSpec[]>([blank(true)]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const adding = target?.mode === 'add-column';

  useEffect(() => {
    if (!target) return;
    setSchema(target.mode === 'add-column' ? target.schema ?? '' : '');
    setTable(target.mode === 'add-column' ? target.table : '');
    setColumns(target.mode === 'add-column' ? [blank()] : [blank(true), blank()]);
    setError(null);
  }, [target]);

  const close = () => setUi({ tableChangeTarget: null });
  const update = (index: number, patch: Partial<ColumnSpec>) => setColumns((current) => current.map((column, at) => at === index ? { ...column, ...patch } : column));
  const submit = async () => {
    const filled = columns.filter((column) => column.name.trim());
    if (!table.trim()) return setError('Name the table.');
    if (!filled.length) return setError('Add at least one named column.');
    setBusy(true); setError(null);
    const change = adding
      ? { kind: 'add_column' as const, schema: schema || undefined, table, column: { ...filled[0], nullable: true, primaryKey: false } }
      : { kind: 'create_table' as const, schema: schema || undefined, table: table.trim(), columns: filled.map((column) => ({ ...column, name: column.name.trim() })) };
    const ok = await apply(change);
    setBusy(false);
    if (ok) close();
  };

  return createPortal(
    <Modal open={target !== null} title={adding ? `Add column to ${table}` : 'New table'} closeLabel="Cancel" onClose={close}
      footer={<><Button variant="ghost" onClick={close}><Icon name="close" /> Cancel</Button><Button data-testid="table-change-submit" disabled={busy} onClick={() => void submit()}><Icon name="check" /> {adding ? 'Add column' : 'Create table'}</Button></>}>
      <div className="table-change-form">
        {error ? <p className="new-row-error" role="alert">{error}</p> : null}
        {!adding ? <div className="table-change-names">
          <label><span>Schema (optional)</span><input data-testid="table-change-schema" value={schema} onChange={(event) => setSchema(event.target.value)} /></label>
          <label><span>Table name</span><input data-testid="table-change-table" value={table} onChange={(event) => setTable(event.target.value)} /></label>
        </div> : null}
        {columns.map((column, index) => <div className="table-change-column" key={index}>
          <label><span>Column</span><input data-testid={`table-change-column-${index}`} value={column.name} onChange={(event) => update(index, { name: event.target.value })} /></label>
          <label><span>Type</span><select data-testid={`table-change-type-${index}`} value={column.columnType} onChange={(event) => update(index, { columnType: event.target.value as ColumnType })}>{TYPES.map((type) => <option key={type.value} value={type.value}>{type.label}</option>)}</select></label>
          {!adding ? <label className="checkbox-label"><input type="checkbox" checked={column.primaryKey} onChange={(event) => update(index, { primaryKey: event.target.checked, nullable: event.target.checked ? false : column.nullable })} /><span><Icon name="check" /></span>Key</label> : null}
          {!adding ? <label className="checkbox-label"><input type="checkbox" disabled={column.primaryKey} checked={column.nullable} onChange={(event) => update(index, { nullable: event.target.checked })} /><span><Icon name="check" /></span>May be empty</label> : <p className="field-hint">An added column may be empty: rows already there have no value for it.</p>}
        </div>)}
        {!adding ? <Button variant="ghost" data-testid="table-change-add-column" onClick={() => setColumns((current) => [...current, blank()])}><Icon name="plus" /> Another column</Button> : null}
      </div>
    </Modal>,
    document.body,
  );
}
