import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Alert, AlertDescription, AlertTitle, type AlertVariant } from './Alert';

const RAW_PALETTE = /\b(?:bg|text|border|ring)-(?:neutral|stone|slate|canvas|white|black)\b/;

const VARIANTS: AlertVariant[] = ['default', 'info', 'success', 'warning', 'destructive'];

describe('Alert', () => {
  it('renders its title and description', () => {
    render(
      <Alert data-testid="alert">
        <AlertTitle data-testid="title">Disk almost full</AlertTitle>
        <AlertDescription data-testid="description">Free some space.</AlertDescription>
      </Alert>
    );

    expect(screen.getByTestId('alert')).toHaveAttribute('data-slot', 'alert');
    expect(screen.getByTestId('title')).toHaveTextContent('Disk almost full');
    expect(screen.getByTestId('description')).toHaveTextContent('Free some space.');
    expect(screen.getByTestId('title')).toHaveAttribute('data-slot', 'alert-title');
    expect(screen.getByTestId('description')).toHaveAttribute('data-slot', 'alert-description');
  });

  it('defaults to the default variant', () => {
    render(<Alert data-testid="alert">Body</Alert>);

    expect(screen.getByTestId('alert')).toHaveAttribute('data-variant', 'default');
  });

  it.each(VARIANTS)('emits data-variant="%s"', variant => {
    render(
      <Alert variant={variant} data-testid="alert">
        Body
      </Alert>
    );

    expect(screen.getByTestId('alert')).toHaveAttribute('data-variant', variant);
  });

  it.each(['destructive', 'warning'] as const)('gives %s an assertive alert role', variant => {
    render(
      <Alert variant={variant} data-testid="alert">
        Body
      </Alert>
    );

    expect(screen.getByTestId('alert')).toHaveAttribute('role', 'alert');
  });

  it.each(['default', 'info', 'success'] as const)('leaves %s without an alert role', variant => {
    render(
      <Alert variant={variant} data-testid="alert">
        Body
      </Alert>
    );

    expect(screen.getByTestId('alert')).not.toHaveAttribute('role');
  });

  it('lets a caller override the role explicitly', () => {
    render(
      <Alert variant="info" role="status" data-testid="alert">
        Body
      </Alert>
    );

    expect(screen.getByTestId('alert')).toHaveAttribute('role', 'status');
  });

  it('forwards rest props and a ref onto the DOM node', () => {
    let node: HTMLDivElement | null = null;
    render(
      <Alert
        ref={el => {
          node = el;
        }}
        id="disk-alert"
        aria-label="Disk"
        data-analytics-id="disk-alert"
        data-testid="alert">
        Body
      </Alert>
    );

    const el = screen.getByTestId('alert');
    expect(node).toBe(el);
    expect(el).toHaveAttribute('id', 'disk-alert');
    expect(el).toHaveAttribute('aria-label', 'Disk');
    expect(el).toHaveAttribute('data-analytics-id', 'disk-alert');
  });

  it('lets a caller className win over the defaults', () => {
    render(
      <Alert className="rounded-none" data-testid="alert">
        Body
      </Alert>
    );

    const cls = screen.getByTestId('alert').className;
    expect(cls).toContain('rounded-none');
    expect(cls).not.toContain('rounded-xl');
  });

  it.each(VARIANTS)('resolves %s to design tokens, never a raw palette class', variant => {
    render(
      <Alert variant={variant} data-testid="alert">
        <AlertTitle data-testid="title">Title</AlertTitle>
        <AlertDescription data-testid="description">Description</AlertDescription>
      </Alert>
    );

    expect(screen.getByTestId('alert').className).not.toMatch(RAW_PALETTE);
    expect(screen.getByTestId('title').className).not.toMatch(RAW_PALETTE);
    expect(screen.getByTestId('description').className).not.toMatch(RAW_PALETTE);
  });
});
