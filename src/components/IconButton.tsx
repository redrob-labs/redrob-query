import type { ButtonHTMLAttributes, ReactNode } from 'react';
import clsx from 'clsx';

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  active?: boolean;
  badge?: number;
  children: ReactNode;
}

export function IconButton({ label, active, badge, children, className, ...props }: Props) {
  return (
    <button className={clsx('icon-button', active && 'is-active', className)} aria-label={label} data-tooltip={label} {...props}>
      {children}{badge ? <span className="icon-badge" aria-label={`${badge} items`}>{badge}</span> : null}
    </button>
  );
}
