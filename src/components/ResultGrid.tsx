import { useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { AlertTriangle, ArrowDown, Braces, Check, ChevronDown, Columns3, Download, Filter, Hash, RefreshCw, Search, TableProperties, TextCursorInput, ToggleLeft } from 'lucide-react';
import clsx from 'clsx';
import type { CellValue, DataType } from '../domain/types';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

const TypeIcon = ({ type }: { type: DataType }) => type === 'number' ? <Hash size={11} /> : type === 'boolean' ? <ToggleLeft size={12} /> : type === 'json' ? <Braces size={12} /> : <TextCursorInput size={11} />;
export const formatCell = (value: CellValue, type: DataType) => {
  if (value === null) return 'NULL';
  if (type === 'number' && typeof value === 'number') return value.toLocaleString('en-US');
  if (type === 'boolean' && typeof value === 'boolean') return value ? 'true' : 'false';
  if (type === 'date' && typeof value === 'string') return value.replace('T', ' ').replace('Z', '');
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
};

export const parseCellInput = (raw: string, type: DataType, nullable = false): CellValue => {
  const value = raw.trim();
  if (nullable && value.toUpperCase() === 'NULL') return null;
  if (type === 'number') {
    const parsed = Number(value.replaceAll(',', ''));
    if (!Number.isFinite(parsed)) throw new Error('Enter a valid number.');
    return parsed;
  }
  if (type === 'boolean') {
    if (value !== 'true' && value !== 'false') throw new Error('Boolean values must be true or false.');
    return value === 'true';
  }
  if (type === 'json') {
    try { return JSON.parse(value) as Record<string, unknown>; }
    catch { throw new Error('Enter valid JSON.'); }
  }
  return raw;
};

export function ResultGrid() {
  const result = useWorkspace((state) => state.result);
  const status = useWorkspace((state) => state.queryStatus);
  const mode = useWorkspace((state) => state.bridge.mode);
  const activeConnectionId = useWorkspace((state) => state.activeConnectionId);
  const error = useWorkspace((state) => state.queryError);
  const mutations = useWorkspace((state) => state.mutations);
  const stageCell = useWorkspace((state) => state.stageCell);
  const notify = useWorkspace((state) => state.notify);
  const runQuery = useWorkspace((state) => state.runQuery);
  const setUi = useWorkspace((state) => state.setUi);
  const [filter, setFilter] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const visibleRows = useMemo(() => {
    if (!result) return [];
    if (!filter.trim()) return result.rows;
    const value = filter.toLowerCase();
    return result.rows.filter((row) => Object.values(row).some((cell) => String(cell).toLowerCase().includes(value)));
  }, [filter, result]);
  const virtualizer = useVirtualizer({ count: visibleRows.length, getScrollElement: () => scrollRef.current, estimateSize: () => 34, overscan: 10 });
  const gridTemplate = result ? `46px ${result.columns.map((column) => `${column.key === 'email' ? 230 : column.key === 'created_at' ? 190 : column.key === 'name' ? 165 : 120}px`).join(' ')}` : '';
  const exportRows = () => {
    if (!result) return;
    const escape = (value: CellValue) => `"${formatCell(value, 'string').replaceAll('"', '""')}"`;
    const csv = [result.columns.map((column) => escape(column.label)).join(','), ...visibleRows.map((row) => result.columns.map((column) => escape(row[column.key])).join(','))].join('\n');
    const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv' }));
    const link = document.createElement('a');
    link.href = url; link.download = 'redrob-results.csv'; link.click(); URL.revokeObjectURL(url);
    notify('success', 'Results exported', `${visibleRows.length} rows saved as CSV.`);
  };

  return (
    <section className="results-shell" aria-label="Query results" data-testid="results-panel">
      <div className="results-tabs">
        <button className="result-tab is-active"><TableProperties size={14} /> Results {result ? <span>{result.rowCount}</span> : null}</button>
        <button className="result-tab" disabled><span className="message-icon">i</span> Messages</button>
        <div className="toolbar-spacer" />
        {status === 'success' && result ? <span className="query-metric"><Check size={12} /> {result.rowCount} rows in {result.durationMs} ms</span> : null}
      </div>
      {status === 'idle' ? activeConnectionId ? <div className="result-empty"><span className="empty-illustration"><Columns3 size={27} /></span><h2>Ready when you are</h2><p>Run the query above to explore {mode === 'demo' ? 'the sample dataset' : 'your active connection'}.</p><button className="secondary-button" onClick={() => void runQuery()}>Run query</button></div> : <div className="result-empty"><span className="empty-illustration"><Columns3 size={27} /></span><h2>Select a connection</h2><p>Choose a saved profile before loading metadata or running a query.</p></div> : null}
      {status === 'loading' ? <div className="result-loading"><div className="loading-grid">{Array.from({ length: 8 }, (_, row) => <div key={row}>{Array.from({ length: 5 }, (_, col) => <span key={col} />)}</div>)}</div><p><span className="spinner" /> {mode === 'demo' ? 'Executing against demo workspace…' : 'Executing against active connection…'}</p></div> : null}
      {status === 'error' ? <div className="result-empty error"><span className="empty-illustration"><AlertTriangle size={25} /></span><h2>Query couldn’t run</h2><p>{error}</p><button className="secondary-button" onClick={() => void runQuery()}><RefreshCw size={14} /> Try again</button></div> : null}
      {status === 'success' && result && result.rows.length === 0 ? <div className="result-empty"><span className="empty-illustration"><Search size={25} /></span><h2>No rows returned</h2><p>Try widening your filters or checking the selected schema.</p></div> : null}
      {status === 'success' && result && result.rows.length > 0 ? <>
        <div className="grid-toolbar">
          <label><Search size={13} /><span className="sr-only">Filter result rows</span><input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter rows…" /></label>
          <button disabled title="Use the row search to filter current results"><Filter size={13} /> Filter</button><button disabled title="Column chooser is not available yet"><Columns3 size={13} /> Columns</button>
          <div className="toolbar-spacer" />
          {mutations.length ? <button className="changes-pill" onClick={() => setUi({ changesOpen: true })}>{mutations.length} staged change{mutations.length === 1 ? '' : 's'}</button> : null}
          <IconButton label="Export results" onClick={exportRows}><Download size={14} /></IconButton>
          <button className="page-size" disabled>100 rows <ChevronDown size={12} /></button>
        </div>
        <div className="data-grid" ref={scrollRef} role="table" aria-rowcount={visibleRows.length}>
          <div className="grid-header" role="row" style={{ gridTemplateColumns: gridTemplate }}>
            <div role="columnheader" className="row-number">#</div>
            {result.columns.map((column) => <div role="columnheader" key={column.key}><TypeIcon type={column.dataType} /><span>{column.label}</span>{column.primaryKey ? <span className="pk">PK</span> : null}<ArrowDown size={11} className="sort-icon" /></div>)}
          </div>
          <div className="grid-body" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const row = visibleRows[virtualRow.index];
              const originalIndex = result.rows.indexOf(row);
              const primaryKey = result.editSource?.primaryKey;
              const rowIdentity = primaryKey ? String(row[primaryKey]) : String(virtualRow.index);
              return <div className="grid-row" role="row" data-index={virtualRow.index} key={rowIdentity} style={{ gridTemplateColumns: gridTemplate, transform: `translateY(${virtualRow.start}px)` }}>
                <div className="row-number" role="cell">{virtualRow.index + 1}</div>
                {result.columns.map((column) => {
                  const editable = Boolean(result.editSource && column.key !== result.editSource.primaryKey);
                  const mutation = mutations.find((item) => item.rowKey === rowIdentity && item.column === column.key);
                  const value = mutation?.nextValue ?? row[column.key];
                  return <div
                    role="cell" key={column.key} className={clsx('grid-cell', column.dataType === 'number' && 'numeric', column.dataType === 'boolean' && 'boolean', value === null && 'null', mutation && 'is-staged', !editable && 'read-only')}
                    contentEditable={editable}
                    suppressContentEditableWarning
                    onBlur={(event) => {
                      if (!editable) return;
                      const raw = event.currentTarget.textContent ?? '';
                      if (raw === formatCell(value, column.dataType)) return;
                      try { stageCell(originalIndex, column.key, parseCellInput(raw, column.dataType, column.nullable)); }
                      catch (parseError) {
                        event.currentTarget.textContent = formatCell(value, column.dataType);
                        notify('error', 'Invalid cell value', parseError instanceof Error ? parseError.message : 'Check the value and try again.');
                      }
                    }}
                    aria-readonly={!editable}
                    aria-label={`${column.label}, row ${virtualRow.index + 1}`}
                  >{formatCell(value, column.dataType)}</div>;
                })}
              </div>;
            })}
          </div>
        </div>
        <div className="grid-footer"><span>Showing {visibleRows.length} of {result.rowCount} rows</span><span className="grid-hint">{result.editSource ? 'Double-click a cell to edit · changes are staged' : 'Editing unavailable · source identity unavailable'}</span><div className="pagination"><button disabled>‹</button><button className="active">1</button><button disabled>›</button></div></div>
      </> : null}
    </section>
  );
}
