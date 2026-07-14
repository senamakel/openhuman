import { fireEvent, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import * as service from '../../../services/memorySourcesService';
import { renderWithProviders } from '../../../test/test-utils';
import { CodingSessionsCard } from '../CodingSessionsCard';

vi.mock('../../../services/memorySourcesService', async () => {
  const actual = await vi.importActual<typeof import('../../../services/memorySourcesService')>(
    '../../../services/memorySourcesService'
  );
  return { ...actual, getCodingSessionStatus: vi.fn(), ingestCodingSessions: vi.fn() };
});

const mockedStatus = vi.mocked(service.getCodingSessionStatus);
const mockedIngest = vi.mocked(service.ingestCodingSessions);

describe('CodingSessionsCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedStatus.mockResolvedValue([
      {
        kind: 'claude_code',
        available: true,
        session_files: 2,
        evidence_units: 4,
        invalid_files: 0,
      },
      { kind: 'codex', available: true, session_files: 3, evidence_units: 7, invalid_files: 0 },
    ]);
  });

  it('shows discovered local session counts', async () => {
    renderWithProviders(<CodingSessionsCard />);

    expect(await screen.findByTestId('coding-session-source-claude_code')).toHaveTextContent(
      '2 sessions · 4 human turns'
    );
    expect(screen.getByTestId('coding-session-source-codex')).toHaveTextContent(
      '3 sessions · 7 human turns'
    );
    expect(screen.getByTestId('coding-sessions-ingest')).toBeEnabled();
  });

  it('ingests incrementally and reports the distilled observations', async () => {
    mockedIngest.mockResolvedValue({
      mode: 'incremental',
      files_seen: 5,
      sessions_processed: 4,
      sessions_skipped: 1,
      sessions_failed: 0,
      evidence_units: 11,
      observations: 6,
      budget_hit: false,
      pack_path: '/workspace/persona/PERSONA.md',
    });
    const onToast = vi.fn();
    renderWithProviders(<CodingSessionsCard onToast={onToast} />);

    fireEvent.click(await screen.findByTestId('coding-sessions-ingest'));

    await waitFor(() => expect(mockedIngest).toHaveBeenCalledWith(false));
    await waitFor(() =>
      expect(onToast).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'success',
          message: '4 sessions produced 6 persona observations.',
        })
      )
    );
  });

  it('keeps ingestion disabled when no human-authored evidence exists', async () => {
    mockedStatus.mockResolvedValue([
      { kind: 'codex', available: false, session_files: 0, evidence_units: 0, invalid_files: 0 },
    ]);
    renderWithProviders(<CodingSessionsCard />);

    expect(await screen.findByText('No local history found')).toBeInTheDocument();
    expect(screen.getByTestId('coding-sessions-ingest')).toBeDisabled();
  });
});
