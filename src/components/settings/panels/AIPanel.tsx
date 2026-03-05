import { useState, useEffect } from 'react';
import { loadSoul, clearSoulCache } from '../../../lib/ai/soul/loader';
import type { SoulConfig } from '../../../lib/ai/soul/types';
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

const AIPanel = () => {
  const { navigateBack } = useSettingsNavigation();
  const [soulConfig, setSoulConfig] = useState<SoulConfig | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>('');

  useEffect(() => {
    loadSoulPreview();
  }, []);

  const loadSoulPreview = async () => {
    setLoading(true);
    setError('');
    try {
      const config = await loadSoul();
      setSoulConfig(config);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to load SOUL configuration';
      setError(message);
    } finally {
      setLoading(false);
    }
  };

  const refreshSoulConfig = async () => {
    setLoading(true);
    setError('');
    try {
      // Clear cache to force fresh load from GitHub/bundled source
      clearSoulCache();
      const config = await loadSoul();
      setSoulConfig(config);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to refresh SOUL configuration';
      setError(message);
    } finally {
      setLoading(false);
    }
  };

  const formatPersonality = (config: SoulConfig): string => {
    return config.personality
      .slice(0, 3)
      .map(p => `${p.trait}: ${p.description}`)
      .join(' • ');
  };

  const formatSafetyRules = (config: SoulConfig): string => {
    return config.safetyRules
      .slice(0, 2)
      .map(r => r.rule)
      .join(' • ');
  };

  return (
    <div className="h-full flex flex-col">
      <SettingsHeader title="AI Configuration" showBackButton={true} onBack={navigateBack} />

      <div className="flex-1 overflow-y-auto px-6 pb-10 space-y-6">
        <section className="space-y-4">
          <h3 className="text-lg font-semibold text-white">SOUL Persona Configuration</h3>
          <p className="text-sm text-gray-400">
            The SOUL system injects persona context into every user message to ensure consistent AI behavior.
          </p>

          {loading && (
            <div className="text-sm text-gray-400 animate-pulse">Loading SOUL configuration...</div>
          )}

          {error && (
            <div className="bg-red-500/10 border border-red-500/40 rounded-lg p-3">
              <div className="text-sm text-red-200">{error}</div>
            </div>
          )}

          {soulConfig && (
            <div className="space-y-3">
              <div className="bg-gray-900 rounded-lg p-4 border border-gray-700 space-y-3">
                <div>
                  <label className="text-xs text-gray-400 uppercase tracking-wide">Identity</label>
                  <div className="text-sm text-green-400 font-medium mt-1">
                    {soulConfig.identity.name}
                  </div>
                  <div className="text-xs text-gray-300 mt-1">
                    {soulConfig.identity.description}
                  </div>
                </div>

                {soulConfig.personality.length > 0 && (
                  <div>
                    <label className="text-xs text-gray-400 uppercase tracking-wide">Personality</label>
                    <div className="text-xs text-gray-300 mt-1 leading-relaxed">
                      {formatPersonality(soulConfig)}
                    </div>
                  </div>
                )}

                {soulConfig.safetyRules.length > 0 && (
                  <div>
                    <label className="text-xs text-gray-400 uppercase tracking-wide">Safety Rules</label>
                    <div className="text-xs text-yellow-300 mt-1 leading-relaxed">
                      {formatSafetyRules(soulConfig)}
                    </div>
                  </div>
                )}

                <div className="flex items-center justify-between pt-2 border-t border-gray-700">
                  <div className="text-xs text-gray-400">
                    Source: {soulConfig.isDefault ? 'Bundled' : 'GitHub'}
                  </div>
                  <div className="text-xs text-gray-400">
                    Loaded: {new Date(soulConfig.loadedAt).toLocaleTimeString()}
                  </div>
                </div>
              </div>

              <button
                onClick={refreshSoulConfig}
                className="text-sm text-blue-400 hover:text-blue-300 transition-colors"
                disabled={loading}
              >
                {loading ? 'Refreshing...' : 'Refresh SOUL Configuration'}
              </button>
            </div>
          )}
        </section>
      </div>
    </div>
  );
};

export default AIPanel;