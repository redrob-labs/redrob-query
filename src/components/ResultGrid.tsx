import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { createPortal } from 'react-dom';
import { Button, Drawer, Loader, Menu, Modal, Textarea } from '@redrob-labs/ui';
import { rowAsJson, rowAsTsv } from './rowExport';
import { Icon } from '../ui/Icon';
import clsx from 'clsx';
import type { CellValue, DataType } from '../domain/types';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';
import { formatShortcut, matches } from '../shortcuts';

const TypeIcon = ({ type }: { type: DataType }) => type === 'number' ? <Icon name="hash" /> : type === 'boolean' ? <Icon name="sliders" /> : type === 'json' ? <Icon name="code" /> : <Icon name="note" />;
export const formatCell = (value: CellValue, type: DataType) => { if (value === null) return 'NULL'; if (type === 'number' && typeof value === 'number') return value.toLocaleString('en-US'); if (type === 'boolean' && typeof value === 'boolean') return value ? 'true' : 'false'; if (type === 'date' && typeof value === 'string') return value.replace('T', ' ').replace('Z', ''); if (typeof value === 'object') return JSON.stringify(value); return String(value); };
export const parseCellInput = (raw: string, type: DataType, nullable = false): CellValue => { const value = raw.trim(); if (nullable && value.toUpperCase() === 'NULL') return null; if (type === 'number') { const parsed = Number(value.replaceAll(',', '')); if (!Number.isFinite(parsed)) throw new Error('Enter a valid number.'); return parsed; } if (type === 'boolean') { if (value !== 'true' && value !== 'false') throw new Error('Boolean values must be true or false.'); return value === 'true'; } if (type === 'json') { try { return JSON.parse(value) as Record<string, unknown>; } catch { throw new Error('Enter valid JSON.'); } } return raw; };
type SortState = { key: string; direction: 'asc' | 'desc' } | null;
const compareCells = (left: CellValue, right: CellValue) => { if (left === right) return 0; if (left === null) return 1; if (right === null) return -1; if (typeof left === 'number' && typeof right === 'number') return left - right; if (typeof left === 'boolean' && typeof right === 'boolean') return Number(left) - Number(right); return String(left).localeCompare(String(right), undefined, { numeric: true, sensitivity: 'base' }); };

export function ResultGrid() {
  const result = useWorkspace((state) => state.result);
  const resultRevision = useWorkspace((state) => state.resultRevision);
  const status = useWorkspace((state) => state.queryStatus);
  const mode = useWorkspace((state) => state.bridge.mode);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const error = useWorkspace((state) => state.queryError);
  const mutations = useWorkspace((state) => state.mutations);
  const pageSize = useWorkspace((state) => state.pageSize);
  const pageOffset = useWorkspace((state) => state.pageOffset);
  const stageCell = useWorkspace((state) => state.stageCell);
  const notify = useWorkspace((state) => state.notify);
  const runQuery = useWorkspace((state) => state.runQuery);
  const nextPage = useWorkspace((state) => state.nextPage);
  const previousPage = useWorkspace((state) => state.previousPage);
  const setPageSize = useWorkspace((state) => state.setPageSize);
  const setUi = useWorkspace((state) => state.setUi);
  const [filter, setFilter] = useState('');
  const [sort, setSort] = useState<SortState>(null);
  const [hiddenColumns, setHiddenColumns] = useState<Set<string>>(new Set());
  const [columnsOpen, setColumnsOpen] = useState(false);
  // The row whose details are open, as shown: a staged edit counts, since that is what the row reads.
  // Shift+Enter on an editable cell opens it in a larger editor (Beekeeper's "Edit in modal"). Saving
  // stages the change exactly as an inline edit does; nothing is written until the change set is applied.
  const [editing, setEditing] = useState<{ rowIndex: number; key: string; label: string; text: string; error?: string } | null>(null);
  const stageDelete = useWorkspace((state) => state.stageDelete);
  const stageInsert = useWorkspace((state) => state.stageInsert);
  // A new row, typed column by column. A field left empty is left out, so the table default (an
  // auto-increment key, a DEFAULT) applies; type NULL to store a null.
  const [newRow, setNewRow] = useState<{ values: Record<string, string>; error?: string } | null>(null);
  const [detailRow, setDetailRow] = useState<{ number: number; values: Record<string, CellValue> } | null>(null);
  const [activeView, setActiveView] = useState<'results' | 'messages'>('results');
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    setFilter('');
    setSort(null);
    setHiddenColumns(new Set());
    setColumnsOpen(false);
    setActiveView('results');
    scrollRef.current?.scrollTo({ top: 0 });
  }, [resultRevision]);
  const visibleColumns = useMemo(() => result?.columns.filter((column) => !hiddenColumns.has(column.key)) ?? [], [hiddenColumns, result]);
  const valueFor = (row: Record<string, CellValue>, key: string) => {
    const primaryKey = result?.editSource?.primaryKey;
    const rowKey = primaryKey ? String(row[primaryKey]) : null;
    return mutations.find((mutation) => rowKey && mutation.rowKey === rowKey && mutation.column === key)?.nextValue ?? row[key];
  };
  const visibleRows = useMemo(() => {
    if (!result) return [];
    const needle = filter.trim().toLowerCase();
    const rows = result.rows.map((row, originalIndex) => ({ row, originalIndex })).filter(({ row }) => !needle || visibleColumns.some((column) => String(valueFor(row, column.key)).toLowerCase().includes(needle)));
    if (!sort) return rows;
    return [...rows].sort((left, right) => compareCells(valueFor(left.row, sort.key), valueFor(right.row, sort.key)) * (sort.direction === 'asc' ? 1 : -1));
  }, [filter, hiddenColumns, mutations, result, sort, visibleColumns]);
  const virtualizer = useVirtualizer({ count: visibleRows.length, getScrollElement: () => scrollRef.current, estimateSize: () => 34, overscan: 10 });
  const gridTemplate = result ? `46px ${visibleColumns.map((column) => `${column.key === 'email' ? 230 : column.key === 'created_at' ? 190 : column.key === 'name' ? 165 : 120}px`).join(' ')}` : '';
  const cycleSort = (key: string) => setSort((current) => !current || current.key !== key ? { key, direction: 'asc' } : current.direction === 'asc' ? { key, direction: 'desc' } : null);
  const exportRows = () => {
    if (!result) return;
    const escape = (value: CellValue) => `"${formatCell(value, 'string').replaceAll('"', '""')}"`;
    const csv = [visibleColumns.map((column) => escape(column.label)).join(','), ...visibleRows.map(({ row }) => visibleColumns.map((column) => escape(valueFor(row, column.key))).join(','))].join('\n');
    const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv' })); const link = document.createElement('a'); link.href = url; link.download = 'redrob-results.csv'; link.click(); URL.revokeObjectURL(url);
    notify('success', 'Results exported', `${visibleRows.length} rows and ${visibleColumns.length} columns saved as CSV.`);
  };
  const start = result?.rows.length ? (result.offset ?? pageOffset) + 1 : 0;
  const end = result ? (result.offset ?? pageOffset) + result.rows.length : 0;
  const pageNumber = Math.floor(pageOffset / pageSize) + 1;
  return (
    <section className="results-shell" aria-label="Query results" data-testid="results-panel">
      <div className="results-tabs"><button className={clsx('result-tab', activeView === 'results' && 'is-active')} onClick={() => setActiveView('results')}><Icon name="layout" /> Results {result ? <span>{result.rowCount}</span> : null}</button><button className={clsx('result-tab', activeView === 'messages' && 'is-active')} onClick={() => setActiveView('messages')} disabled={!result}><Icon name="info" /> Messages</button><div className="toolbar-spacer" />{status === 'success' && result ? <span className="query-metric"><Icon name="check" /> {result.rowCount} rows in {result.durationMs} ms</span> : null}</div>
      {status === 'idle' ? activeConnectionId ? <div className="result-empty"><span className="empty-illustration"><Icon name="columns" size={24} /></span><h2>Ready when you are</h2><p>Run the query above to explore {mode === 'demo' ? 'the sample dataset' : 'your active connection'}.</p><button className="secondary-button" onClick={() => void runQuery()}><Icon name="play" /> Run query</button></div> : <div className="result-empty"><span className="empty-illustration"><Icon name="columns" size={24} /></span><h2>Select a connection</h2><p>Choose and connect a saved profile before loading metadata or running a query.</p></div> : null}
      {status === 'loading' ? <div className="result-loading"><div className="loading-grid">{Array.from({ length: 8 }, (_, row) => <div key={row}>{Array.from({ length: 5 }, (_, col) => <span key={col} />)}</div>)}</div><p><Loader size="md" label="Loading rows" /> {mode === 'demo' ? 'Executing against demo workspace…' : 'Executing against active connection…'}</p></div> : null}
      {status === 'error' ? <div className="result-empty error"><span className="empty-illustration"><Icon name="warning" size={24} /></span><h2>Query couldn’t run</h2><p>{error}</p><button className="secondary-button" onClick={() => void runQuery()}><Icon name="refresh" /> Try again</button></div> : null}
      {status === 'success' && result && activeView === 'messages' ? <div className="messages-view"><h2>Execution messages</h2><p>{result.message || 'Query completed successfully.'}</p><dl><div><dt>Returned</dt><dd>{result.rowCount} rows</dd></div><div><dt>Elapsed</dt><dd>{result.durationMs} ms</dd></div><div><dt>Range</dt><dd>{start ? `${start}–${end}` : 'No rows'}{result.nextOffset !== null && result.nextOffset !== undefined ? ' · more available' : ' · end of results'}</dd></div><div><dt>Page limit</dt><dd>{result.limit ?? pageSize}</dd></div></dl></div> : null}
      {status === 'success' && result && activeView === 'results' && result.rows.length === 0 ? <div className="result-empty"><span className="empty-illustration"><Icon name="search" size={24} /></span><h2>No rows returned</h2><p>Try widening your filters or checking the selected schema.</p></div> : null}
      {status === 'success' && result && activeView === 'results' && result.rows.length > 0 ? <>
        <div className="grid-toolbar"><label><Icon name="search" /><span className="sr-only">Filter current page</span><input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter current page…" /></label>{filter ? <button onClick={() => setFilter('')}><Icon name="close" /> Clear filter</button> : null}<button data-testid="add-row" disabled={!result.editSource} title={result.editSource ? undefined : 'This result cannot be edited'} onClick={() => setNewRow({ values: {} })}><Icon name="plus" /> Add row</button><div className="columns-menu"><button onClick={() => setColumnsOpen((value) => !value)}><Icon name="columns" /> Columns</button>{columnsOpen ? <div className="columns-popover">{result.columns.map((column) => <label key={column.key}><input type="checkbox" checked={!hiddenColumns.has(column.key)} onChange={() => setHiddenColumns((current) => { const next = new Set(current); if (next.has(column.key)) next.delete(column.key); else if (current.size < result.columns.length - 1) next.add(column.key); return next; })} /> {column.label}</label>)}</div> : null}</div><div className="toolbar-spacer" />{mutations.length ? <button className="changes-pill" onClick={() => setUi({ changesOpen: true })}><Icon name="compare" /> {mutations.length} staged change{mutations.length === 1 ? '' : 's'}</button> : null}<IconButton label="Export visible results" onClick={exportRows}><Icon name="download" /></IconButton><label className="page-size"><span className="sr-only">Rows per page</span><select aria-label="Rows per page" value={pageSize} onChange={(event) => void setPageSize(Number(event.target.value) as 25 | 50 | 100 | 250)}>{[25, 50, 100, 250].map((size) => <option key={size} value={size}>{size} rows</option>)}</select></label></div>
        <div className="data-grid" ref={scrollRef} role="table" aria-rowcount={visibleRows.length}><div className="grid-header" role="row" style={{ gridTemplateColumns: gridTemplate }}><div role="columnheader" className="row-number">#</div>{visibleColumns.map((column) => <div role="columnheader" key={column.key}><button className="column-sort" onClick={() => cycleSort(column.key)} aria-label={`Sort ${column.label}${sort?.key === column.key ? ` ${sort.direction}` : ''}`}><TypeIcon type={column.dataType} /><span>{column.label}</span>{column.primaryKey ? <span className="pk">PK</span> : null}{sort?.key === column.key ? sort.direction === 'asc' ? <Icon name="arrowUp" className="sort-icon" /> : <Icon name="arrowDown" className="sort-icon" /> : <span className="sort-placeholder" />}</button><Menu
          label={<><Icon name="more" /><span className="sr-only">Column actions for {column.label}</span></>}
          variant="ghost" size="sm" align={column.key === visibleColumns.at(-1)?.key ? 'right' : 'left'} className="column-menu"
          items={[
            { id: 'asc', label: 'Sort ascending', icon: <Icon name="arrowUp" />, onSelect: () => setSort({ key: column.key, direction: 'asc' }) },
            { id: 'desc', label: 'Sort descending', icon: <Icon name="arrowDown" />, onSelect: () => setSort({ key: column.key, direction: 'desc' }) },
            { id: 'unsort', label: 'Clear sort', icon: <Icon name="minus" />, disabled: sort?.key !== column.key, onSelect: () => setSort(null) },
            { type: 'separator' },
            // The last visible column cannot be hidden: a grid with no columns has no way back but Columns.
            { id: 'hide', label: 'Hide column', icon: <Icon name="eyeOff" />, disabled: visibleColumns.length === 1, onSelect: () => setHiddenColumns((current) => new Set(current).add(column.key)) },
            { id: 'copy', label: 'Copy column name', icon: <Icon name="copy" />, onSelect: () => { void navigator.clipboard.writeText(column.label).then(() => notify('success', 'Column name copied', column.label), () => notify('error', 'Copy unavailable', 'Select the name and copy it manually.')) } },
          ]} /></div>)}</div>
          <div className="grid-body" style={{ height: virtualizer.getTotalSize() }}>{virtualizer.getVirtualItems().map((virtualRow) => { const { row, originalIndex } = visibleRows[virtualRow.index]; const primaryKey = result.editSource?.primaryKey; const rowIdentity = primaryKey ? String(row[primaryKey]) : String(originalIndex); return <div className={clsx('grid-row', mutations.some((item) => item.kind === 'delete' && item.rowKey === rowIdentity) && 'is-deleted')} role="row" data-index={virtualRow.index} key={rowIdentity} style={{ gridTemplateColumns: gridTemplate, transform: `translateY(${virtualRow.start}px)` }}><div className="row-number" role="cell"><span>{originalIndex + 1}</span>{(() => {
            const shown: Record<string, CellValue> = Object.fromEntries(result.columns.map((column) => [column.key, mutations.find((item) => item.rowKey === rowIdentity && item.column === column.key)?.nextValue ?? row[column.key]]));
            const copy = (text: string, what: string) => { void navigator.clipboard.writeText(text).then(() => notify('success', `${what} copied`), () => notify('error', 'Copy unavailable', 'Open See details and copy from there.')); };
            return <Menu
              label={<><Icon name="more" /><span className="sr-only">Row {originalIndex + 1} actions</span></>}
              variant="ghost" size="sm" align="left" className="row-menu"
              items={[
                { id: 'details', label: 'See details', icon: <Icon name="panelRight" />, onSelect: () => setDetailRow({ number: originalIndex + 1, values: shown }) },
                { type: 'separator' },
                { id: 'json', label: 'Copy row as JSON', icon: <Icon name="copy" />, onSelect: () => copy(rowAsJson(result.columns, (key) => shown[key]), 'Row as JSON') },
                { id: 'tsv', label: 'Copy row as TSV', icon: <Icon name="columns" />, onSelect: () => copy(rowAsTsv(result.columns, (key) => shown[key]), 'Row as TSV') },
                { type: 'separator' },
                // Staged like a cell edit: nothing is removed until the change set is reviewed and applied.
                { id: 'delete', label: 'Delete row', icon: <Icon name="trash" />, tone: 'danger', disabled: !result.editSource, onSelect: () => stageDelete(originalIndex) },
              ]} />;
          })()}</div>{visibleColumns.map((column) => { const editable = Boolean(result.editSource && column.key !== result.editSource.primaryKey); const mutation = mutations.find((item) => item.rowKey === rowIdentity && item.column === column.key); const value = mutation?.nextValue ?? row[column.key]; return <div role="cell" key={column.key} className={clsx('grid-cell', column.dataType === 'number' && 'numeric', column.dataType === 'boolean' && 'boolean', value === null && 'null', mutation && 'is-staged', !editable && 'read-only')} contentEditable={editable} suppressContentEditableWarning onKeyDown={(event) => { if (!editable || !matches(event, 'editCell')) return; event.preventDefault(); setEditing({ rowIndex: originalIndex, key: column.key, label: column.label, text: column.dataType === 'json' && value !== null && typeof value === 'object' ? JSON.stringify(value, null, 2) : formatCell(value, column.dataType) }); }} onBlur={(event) => { if (!editable) return; const raw = event.currentTarget.textContent ?? ''; if (raw === formatCell(value, column.dataType)) return; try { stageCell(originalIndex, column.key, parseCellInput(raw, column.dataType, column.nullable)); } catch (parseError) { event.currentTarget.textContent = formatCell(value, column.dataType); notify('error', 'Invalid cell value', parseError instanceof Error ? parseError.message : 'Check the value and try again.'); } }} aria-readonly={!editable} aria-label={`${column.label}, result row ${originalIndex + 1}`}>{formatCell(value, column.dataType)}</div>; })}</div>; })}</div></div>
        <div className="grid-footer"><span>{filter ? `${visibleRows.length} matching rows on this page` : start ? `Showing ${start}–${end}` : 'No rows'} · {result.nextOffset !== null && result.nextOffset !== undefined ? 'more available' : 'end of results'}</span><span className="grid-hint">{result.editSource ? `Double-click a cell to edit · ${formatShortcut('editCell')} for a larger editor · changes are staged` : 'Editing unavailable · source identity unavailable'}</span><div className="pagination"><button aria-label="Previous page" disabled={pageOffset === 0} onClick={() => void previousPage()}><Icon name="chevronLeft" /></button><button className="active" aria-label={`Page ${pageNumber}`}>{pageNumber}</button><button aria-label="Next page" disabled={result.nextOffset === null || result.nextOffset === undefined} onClick={() => void nextPage()}><Icon name="chevronRight" /></button></div></div>
      </> : null}
      {/* Every column, hidden ones too, with the whole value: a cell shows one line and an ellipsis. Portalled
          to the body: inside the results pane it stacked under the AI panel beside it. */}
      {createPortal(<Drawer open={detailRow !== null} side="right" title={detailRow ? `Row ${detailRow.number}` : ''} description="Every column of this row, in full." closeLabel="Close row details" onClose={() => setDetailRow(null)}>
        {detailRow ? <dl className="row-details">{(result?.columns ?? []).map((column) => { const cell = detailRow.values[column.key]; return <div key={column.key}><dt>{column.label}</dt><dd className={clsx(cell === null && 'null')}>{cell === null ? 'NULL' : typeof cell === 'object' ? JSON.stringify(cell, null, 2) : String(cell)}</dd></div>; })}</dl> : null}
      </Drawer>, document.body)}
      {createPortal(<Modal open={editing !== null} title={editing ? `Edit ${editing.label}` : ''} closeLabel="Cancel editing" onClose={() => setEditing(null)} footer={<><Button variant="ghost" onClick={() => setEditing(null)}><Icon name="close" /> Cancel</Button><Button onClick={() => {
        if (!editing || !result) return; const column = result.columns.find((item) => item.key === editing.key); if (!column) return;
        try { stageCell(editing.rowIndex, editing.key, parseCellInput(editing.text, column.dataType, column.nullable)); setEditing(null); }
        catch (parseError) { setEditing({ ...editing, error: parseError instanceof Error ? parseError.message : 'Check the value and try again.' }); }
      }}><Icon name="check" /> Stage change</Button></>}>
        {editing ? <Textarea aria-label={`New value for ${editing.label}`} rows={10} value={editing.text} error={editing.error} autoFocus onChange={(event) => setEditing({ ...editing, text: event.target.value, error: undefined })} /> : null}
      </Modal>, document.body)}
      {createPortal(<Modal open={newRow !== null} title="Add row" closeLabel="Cancel the new row" onClose={() => setNewRow(null)} footer={<><Button variant="ghost" onClick={() => setNewRow(null)}><Icon name="close" /> Cancel</Button><Button data-testid="stage-new-row" onClick={() => {
        if (!newRow || !result) return;
        try {
          const values = Object.fromEntries(result.columns.filter((column) => (newRow.values[column.key] ?? '') !== '').map((column) => [column.key, parseCellInput(newRow.values[column.key], column.dataType, column.nullable)]));
          if (!Object.keys(values).length) throw new Error('Fill in at least one column.');
          stageInsert(values); setNewRow(null);
        } catch (parseError) { setNewRow({ ...newRow, error: parseError instanceof Error ? parseError.message : 'Check the values and try again.' }); }
      }}><Icon name="check" /> Stage new row</Button></>}>
        {newRow && result ? <div className="new-row-fields">{newRow.error ? <p className="new-row-error" role="alert">{newRow.error}</p> : null}{result.columns.map((column) => <label key={column.key}><span>{column.label}{column.primaryKey ? ' (key — leave empty to let the database assign it)' : ''}</span><input data-testid={`new-row-${column.key}`} value={newRow.values[column.key] ?? ''} onChange={(event) => setNewRow({ values: { ...newRow.values, [column.key]: event.target.value } })} /></label>)}</div> : null}
      </Modal>, document.body)}
    </section>
  );
}
