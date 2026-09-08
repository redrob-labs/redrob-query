import '@testing-library/jest-dom/vitest';
import React from 'react';
import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/react';

afterEach(() => {
  cleanup();
  localStorage.clear();
});

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
Object.defineProperty(globalThis, 'ResizeObserver', { value: ResizeObserverMock, writable: true });
Object.defineProperty(HTMLElement.prototype, 'scrollTo', { value: vi.fn(), writable: true });
Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', { value: vi.fn(), writable: true });

vi.mock('@monaco-editor/react', () => ({
  default: ({ value, onChange }: { value?: string; onChange?: (value: string) => void }) => React.createElement('textarea', {
    'aria-label': 'SQL editor',
    'data-testid': 'monaco-editor',
    value: value ?? '',
    onChange: (event: React.ChangeEvent<HTMLTextAreaElement>) => onChange?.(event.target.value),
  }),
}));
