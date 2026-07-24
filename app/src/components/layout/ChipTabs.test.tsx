import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import ChipTabs, { type ChipTabItem } from './ChipTabs';

type TabId = 'one' | 'two' | 'three';

const items: ChipTabItem<TabId>[] = [
  { id: 'one', label: 'One' },
  { id: 'two', label: 'Two' },
  { id: 'three', label: 'Three' },
];

describe('ChipTabs', () => {
  it('renders a tablist with one chip per item by default', () => {
    render(<ChipTabs items={items} value="one" onChange={() => {}} ariaLabel="Sections" />);

    const list = screen.getByRole('tablist', { name: 'Sections' });
    expect(list).toBeInTheDocument();
    expect(screen.getAllByRole('tab')).toHaveLength(3);
  });

  it('marks the active chip with aria-selected', () => {
    render(<ChipTabs items={items} value="two" onChange={() => {}} testIdPrefix="t" />);

    expect(screen.getByTestId('t-two')).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByTestId('t-one')).toHaveAttribute('aria-selected', 'false');
    expect(screen.getByTestId('t-three')).toHaveAttribute('aria-selected', 'false');
  });

  it('uses roving tabIndex for keyboard navigation', () => {
    render(<ChipTabs items={items} value="two" onChange={() => {}} testIdPrefix="t" />);

    expect(screen.getByTestId('t-two')).toHaveAttribute('tabindex', '0');
    expect(screen.getByTestId('t-one')).toHaveAttribute('tabindex', '-1');
    expect(screen.getByTestId('t-three')).toHaveAttribute('tabindex', '-1');
  });

  it('connects a tab to its panel through stable IDs', () => {
    render(
      <>
        <ChipTabs
          items={[{ id: 'one', label: 'One', controls: 'panel-one', labelledBy: 'tab-one' }]}
          value="one"
          onChange={() => {}}
        />
        <div id="panel-one" role="tabpanel" aria-labelledby="tab-one">
          First panel
        </div>
      </>
    );

    const tab = screen.getByRole('tab', { name: 'One' });
    const panel = screen.getByRole('tabpanel', { name: 'One' });
    expect(tab).toHaveAttribute('id', 'tab-one');
    expect(tab).toHaveAttribute('aria-controls', 'panel-one');
    expect(panel).toHaveAttribute('aria-labelledby', tab.id);
  });

  it('reduces chip padding in compact mode', () => {
    const { rerender } = render(
      <ChipTabs items={items} value="one" onChange={() => {}} testIdPrefix="t" />
    );
    expect(screen.getByTestId('t-one')).toHaveClass('px-3', 'py-1');

    rerender(<ChipTabs items={items} value="one" onChange={() => {}} testIdPrefix="t" compact />);
    expect(screen.getByTestId('t-one')).toHaveClass('px-2', 'py-0.5');
    expect(screen.getByTestId('t-one')).not.toHaveClass('px-3', 'py-1');
  });

  it('emits onChange with the clicked chip id', () => {
    const onChange = vi.fn();
    render(<ChipTabs items={items} value="one" onChange={onChange} testIdPrefix="t" />);

    fireEvent.click(screen.getByTestId('t-three'));
    expect(onChange).toHaveBeenCalledWith('three');
  });

  it('uses an explicit per-item testId over the prefix', () => {
    render(
      <ChipTabs
        items={[{ id: 'one', label: 'One', testId: 'custom-chip' }]}
        value="one"
        onChange={() => {}}
        testIdPrefix="t"
      />
    );

    expect(screen.getByTestId('custom-chip')).toBeInTheDocument();
    expect(screen.queryByTestId('t-one')).not.toBeInTheDocument();
  });

  it('renders navigation semantics with aria-current when as="nav"', () => {
    render(<ChipTabs items={items} value="two" onChange={() => {}} as="nav" ariaLabel="Sub nav" />);

    expect(screen.getByRole('navigation', { name: 'Sub nav' })).toBeInTheDocument();
    // No tab roles in nav mode.
    expect(screen.queryAllByRole('tab')).toHaveLength(0);
    expect(screen.getByText('Two')).toHaveAttribute('aria-current', 'page');
    expect(screen.getByText('One')).not.toHaveAttribute('aria-current');
    expect(screen.getByText('Two')).not.toHaveAttribute('tabindex');
  });
});
