import { configureStore } from '@reduxjs/toolkit';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Provider } from 'react-redux';
import { MemoryRouter } from 'react-router-dom';
import { REHYDRATE } from 'redux-persist';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import mascotReducer, {
  DEFAULT_MASCOT_COLOR,
  setMascotColor,
  setMascotVoiceId,
  setSelectedMascotId,
} from '../../../../store/mascotSlice';
import MascotPanel from '../MascotPanel';

const { mockNavigateBack, fetchMascotListMock, getCachedMascotDetailMock, synthesizeSpeechMock } =
  vi.hoisted(() => ({
    mockNavigateBack: vi.fn(),
    fetchMascotListMock: vi.fn(),
    getCachedMascotDetailMock: vi.fn(),
    synthesizeSpeechMock: vi.fn(),
  }));

vi.mock('../../../../services/mascotService', () => ({
  fetchMascotList: (...args: unknown[]) => fetchMascotListMock(...args),
  getCachedMascotDetail: (...args: unknown[]) => getCachedMascotDetailMock(...args),
}));

vi.mock('../../../../features/human/voice/ttsClient', () => ({
  synthesizeSpeech: (...args: unknown[]) => synthesizeSpeechMock(...args),
}));

vi.mock('../../../../features/human/Mascot/backend/BackendMascot', () => ({
  BackendMascot: ({ mascot }: { mascot: { id: string } }) => (
    <div data-testid={`backend-mascot-preview-${mascot.id}`} />
  ),
}));

vi.mock('../../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({
    navigateBack: mockNavigateBack,
    breadcrumbs: [{ label: 'Settings' }],
  }),
}));

function buildStore() {
  return configureStore({ reducer: { mascot: mascotReducer } });
}

function renderPanel(store = buildStore()) {
  return {
    store,
    ...render(
      <Provider store={store}>
        <MemoryRouter>
          <MascotPanel />
        </MemoryRouter>
      </Provider>
    ),
  };
}

describe('MascotPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    fetchMascotListMock.mockResolvedValue([]);
    getCachedMascotDetailMock.mockResolvedValue(null);
  });

  it('renders a radio swatch for each supported color', () => {
    renderPanel();
    expect(screen.getByRole('radiogroup', { name: 'OpenHuman color' })).toBeInTheDocument();
    for (const label of ['Yellow', 'Burgundy', 'Black', 'Navy', 'Green']) {
      expect(screen.getByRole('radio', { name: label })).toBeInTheDocument();
    }
  });

  it('marks the currently selected color as aria-checked', () => {
    const store = buildStore();
    store.dispatch(setMascotColor('navy'));
    renderPanel(store);
    expect(screen.getByRole('radio', { name: 'Navy' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('radio', { name: 'Yellow' })).toHaveAttribute('aria-checked', 'false');
  });

  it('dispatches setMascotColor when a swatch is clicked', () => {
    const { store } = renderPanel();
    fireEvent.click(screen.getByRole('radio', { name: 'Burgundy' }));
    expect(store.getState().mascot.color).toBe('burgundy');
  });

  it('is a no-op when clicking the already-selected color', () => {
    const store = buildStore();
    store.dispatch(setMascotColor('green'));
    const dispatchSpy = vi.spyOn(store, 'dispatch');
    renderPanel(store);
    fireEvent.click(screen.getByRole('radio', { name: 'Green' }));
    // No additional dispatches beyond what React-Redux did to subscribe.
    expect(dispatchSpy).not.toHaveBeenCalled();
    expect(store.getState().mascot.color).toBe('green');
  });

  it('invokes navigateBack from the header back button', () => {
    renderPanel();
    fireEvent.click(screen.getByLabelText('Back'));
    expect(mockNavigateBack).toHaveBeenCalledTimes(1);
  });
});

// Batch-5: rehydrate cases + unknown-color fallback (issue#1651, pr#1667)
describe('MascotPanel — mascotSlice rehydrate guard', () => {
  it('restores a known persisted color from a REHYDRATE action', () => {
    const store = configureStore({ reducer: { mascot: mascotReducer } });
    store.dispatch({ type: REHYDRATE, key: 'mascot', payload: { color: 'burgundy' } });
    expect(store.getState().mascot.color).toBe('burgundy');
  });

  it('falls back to yellow when REHYDRATE contains an unknown color string', () => {
    const store = configureStore({ reducer: { mascot: mascotReducer } });
    store.dispatch({ type: REHYDRATE, key: 'mascot', payload: { color: 'hot-pink' } });
    expect(store.getState().mascot.color).toBe(DEFAULT_MASCOT_COLOR);
  });

  it('falls back to yellow when REHYDRATE payload is missing the color field', () => {
    const store = configureStore({ reducer: { mascot: mascotReducer } });
    store.dispatch({ type: REHYDRATE, key: 'mascot', payload: {} });
    expect(store.getState().mascot.color).toBe(DEFAULT_MASCOT_COLOR);
  });

  it('falls back to yellow when REHYDRATE payload is null', () => {
    const store = configureStore({ reducer: { mascot: mascotReducer } });
    store.dispatch({ type: REHYDRATE, key: 'mascot', payload: null });
    expect(store.getState().mascot.color).toBe(DEFAULT_MASCOT_COLOR);
  });

  it('ignores REHYDRATE actions for other slice keys', () => {
    const store = configureStore({ reducer: { mascot: mascotReducer } });
    store.dispatch(setMascotColor('navy'));
    store.dispatch({ type: REHYDRATE, key: 'someOtherSlice', payload: { color: 'green' } });
    // Should remain navy — we only handle key === 'mascot'.
    expect(store.getState().mascot.color).toBe('navy');
  });

  it('renders the rehydrated color as selected in the panel', () => {
    const store = configureStore({ reducer: { mascot: mascotReducer } });
    store.dispatch({ type: REHYDRATE, key: 'mascot', payload: { color: 'green' } });
    render(
      <Provider store={store}>
        <MemoryRouter>
          <MascotPanel />
        </MemoryRouter>
      </Provider>
    );
    expect(screen.getByRole('radio', { name: 'Green' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('radio', { name: 'Yellow' })).toHaveAttribute('aria-checked', 'false');
  });

  describe('backend mascot library', () => {
    const summary = {
      id: 'yellow',
      name: 'Yellow',
      version: '1.0.0',
      description: '',
      states: [{ id: 'idle', label: 'Idle', description: '' }],
      hasVisemes: true,
    };
    const detail = {
      id: 'yellow',
      name: 'Yellow',
      version: '1.0.0',
      description: '',
      viewBox: '0 0 1 1',
      defaultState: 'idle',
      variables: [],
      states: [{ id: 'idle', label: 'Idle', description: '', svg: '<svg/>' }],
      visemes: [],
    };

    it('renders the picker entries returned by the API', async () => {
      fetchMascotListMock.mockResolvedValueOnce([summary]);
      renderPanel();
      expect(await screen.findByTestId('backend-mascot-yellow')).toBeInTheDocument();
      // Default-row (local) sentinel
      expect(screen.getByText(/Local OpenHuman/)).toBeInTheDocument();
    });

    it('shows a friendly empty state when the library is empty', async () => {
      fetchMascotListMock.mockResolvedValueOnce([]);
      renderPanel();
      expect(
        await screen.findByText(/No OpenHuman characters are available yet/i)
      ).toBeInTheDocument();
    });

    it('shows an error when the library endpoint rejects', async () => {
      fetchMascotListMock.mockRejectedValueOnce(new Error('offline'));
      renderPanel();
      expect(
        await screen.findByText(/OpenHuman library unavailable: offline/i)
      ).toBeInTheDocument();
    });

    it('dispatches setSelectedMascotId when a backend mascot is picked', async () => {
      fetchMascotListMock.mockResolvedValueOnce([summary]);
      getCachedMascotDetailMock.mockResolvedValueOnce(detail);
      const { store } = renderPanel();
      const row = await screen.findByTestId('backend-mascot-yellow');
      fireEvent.click(row);
      expect(store.getState().mascot.selectedMascotId).toBe('yellow');
    });

    it('loads + previews the active backend mascot detail', async () => {
      const store = buildStore();
      store.dispatch(setSelectedMascotId('yellow'));
      fetchMascotListMock.mockResolvedValueOnce([summary]);
      getCachedMascotDetailMock.mockResolvedValueOnce(detail);
      renderPanel(store);
      expect(await screen.findByTestId('backend-mascot-preview-yellow')).toBeInTheDocument();
      expect(getCachedMascotDetailMock).toHaveBeenCalledWith('yellow');
    });

    it('clearing the selection returns to the local default', async () => {
      const store = buildStore();
      store.dispatch(setSelectedMascotId('yellow'));
      fetchMascotListMock.mockResolvedValueOnce([summary]);
      getCachedMascotDetailMock.mockResolvedValueOnce(detail);
      renderPanel(store);
      const localRow = await screen.findByText(/Local OpenHuman/);
      fireEvent.click(localRow);
      expect(store.getState().mascot.selectedMascotId).toBeNull();
    });
  });
});

// Voice picker — the gender filter, locale-default toggle, preset
// dropdown, custom-paste editor, reset, and preview all dispatch
// through `mascotSlice`. These tests pin the UI surface so a future
// refactor that drifts the names / data-testids fails loudly.
describe('MascotPanel — voice section', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    fetchMascotListMock.mockResolvedValue([]);
    getCachedMascotDetailMock.mockResolvedValue(null);
    synthesizeSpeechMock.mockResolvedValue({
      audio_base64: 'AAA=',
      audio_mime: 'audio/mpeg',
      visemes: [],
    });
  });

  it('renders gender radios with the default ("male") checked', () => {
    renderPanel();
    const male = screen.getByTestId('mascot-voice-gender-male');
    const female = screen.getByTestId('mascot-voice-gender-female');
    expect(male).toHaveAttribute('aria-checked', 'true');
    expect(female).toHaveAttribute('aria-checked', 'false');
  });

  it('clicking the female radio dispatches setMascotVoiceGender', () => {
    const { store } = renderPanel();
    fireEvent.click(screen.getByTestId('mascot-voice-gender-female'));
    expect(store.getState().mascot.voiceGender).toBe('female');
  });

  it('toggling the locale-default checkbox flips the slice', () => {
    const { store } = renderPanel();
    const checkbox = screen.getByTestId('mascot-voice-locale-default') as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
    fireEvent.click(checkbox);
    expect(store.getState().mascot.voiceUseLocaleDefault).toBe(true);
    // And the picker becomes disabled.
    expect(screen.getByTestId('mascot-voice-select')).toBeDisabled();
  });

  it('selecting a preset dispatches setMascotVoiceId with the chosen id', () => {
    const { store } = renderPanel();
    const select = screen.getByTestId('mascot-voice-select') as HTMLSelectElement;
    // Adam — male preset, visible under the default ("male") gender
    // filter without needing to flip the radio first.
    fireEvent.change(select, { target: { value: 'pNInz6obpgDQGcFmaJgB' } });
    expect(store.getState().mascot.voiceId).toBe('pNInz6obpgDQGcFmaJgB');
  });

  it('switching to "Other (paste voice id)…" reveals the custom paste editor', () => {
    renderPanel();
    const select = screen.getByTestId('mascot-voice-select') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: '__custom__' } });
    expect(screen.getByTestId('mascot-voice-input')).toBeInTheDocument();
  });

  it('save-paste writes the trimmed custom id into the slice', () => {
    const { store } = renderPanel();
    fireEvent.change(screen.getByTestId('mascot-voice-select') as HTMLSelectElement, {
      target: { value: '__custom__' },
    });
    const input = screen.getByTestId('mascot-voice-input') as HTMLInputElement;
    fireEvent.change(input, { target: { value: '   pasted-voice-id   ' } });
    fireEvent.click(screen.getByTestId('mascot-voice-save-paste'));
    expect(store.getState().mascot.voiceId).toBe('pasted-voice-id');
  });

  it('reset clears the voiceId override + disables the reset button afterwards', () => {
    const store = buildStore();
    store.dispatch(setMascotVoiceId('custom-id'));
    renderPanel(store);
    const reset = screen.getByTestId('mascot-voice-reset');
    expect(reset).not.toBeDisabled();
    fireEvent.click(reset);
    expect(store.getState().mascot.voiceId).toBeNull();
    expect(screen.getByTestId('mascot-voice-reset')).toBeDisabled();
  });

  it('preview calls synthesizeSpeech with the effective voice id', async () => {
    const store = buildStore();
    store.dispatch(setMascotVoiceId('pNInz6obpgDQGcFmaJgB')); // Adam
    renderPanel(store);
    fireEvent.click(screen.getByTestId('mascot-voice-preview'));
    await waitFor(() =>
      expect(synthesizeSpeechMock).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ voiceId: 'pNInz6obpgDQGcFmaJgB' })
      )
    );
  });

  it('surfaces a preview error banner without dropping the selection', async () => {
    synthesizeSpeechMock.mockRejectedValueOnce(new Error('Backend unreachable'));
    const store = buildStore();
    store.dispatch(setMascotVoiceId('EXAVITQu4vr4xnSDxMaL'));
    renderPanel(store);
    fireEvent.click(screen.getByTestId('mascot-voice-preview'));
    const banner = await screen.findByTestId('mascot-voice-preview-error');
    expect(banner.textContent).toContain('Backend unreachable');
    // Stored selection survives the failed preview.
    expect(store.getState().mascot.voiceId).toBe('EXAVITQu4vr4xnSDxMaL');
  });

  it('keeps the active preset visible after flipping the gender filter', () => {
    const store = buildStore();
    // Adam is male; user flips to female filter → Adam should still
    // appear as a valid <option> so the controlled <select> doesn't
    // desync from its model.
    store.dispatch(setMascotVoiceId('pNInz6obpgDQGcFmaJgB'));
    renderPanel(store);
    fireEvent.click(screen.getByTestId('mascot-voice-gender-female'));
    const select = screen.getByTestId('mascot-voice-select') as HTMLSelectElement;
    expect(select.value).toBe('pNInz6obpgDQGcFmaJgB');
  });
});
