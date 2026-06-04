/**
 * Model Council tab — configure a small council of agent-flavored model seats,
 * ask one question, then let a judge model synthesize the responses.
 *
 * The Rust core still owns orchestration through `openhuman.model_council_run`.
 * This surface gives each seat an agent profile, Rive presence, and council
 * settings, then resolves the roster to model ids for the existing RPC.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';

import {
  getMascotPalette,
  hexToArgbInt,
  type MascotFace,
  RiveMascot,
} from '../../features/human/Mascot';
import { useT } from '../../lib/i18n/I18nContext';
import { BubbleMarkdown } from '../../pages/conversations/components/AgentMessageBubble';
import {
  type CouncilMemberResult,
  modelCouncilApi,
  type ModelCouncilResult,
} from '../../services/api/modelCouncilApi';
import {
  type AgentProfilesStatus,
  loadAgentProfiles,
  selectAgentProfiles,
} from '../../store/agentProfileSlice';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import type { AgentProfile } from '../../types/agentProfile';

/** Matches the server-side MAX_COUNCIL_MEMBERS cap. */
const MAX_MEMBERS = 5;
const MIN_MEMBERS = 1;
const MAX_DEBATE_ROUNDS = 4;
const MIN_DEBATE_ROUNDS = 2;
const ESTIMATED_USD_PER_1K_TOKENS = 0.003;

type SeatMode = 'default' | 'profile' | 'custom';

interface CouncilSeat {
  id: number;
  mode: SeatMode;
  profileId: string;
  name: string;
  model: string;
  brief: string;
}

interface ResolvedSeat {
  label: string;
  model: string;
  brief: string;
}

interface LiveMemberThought {
  status: 'pending' | 'answered' | 'failed';
  member: CouncilMemberResult | null;
  turns: CouncilDebateTurn[];
}

interface CouncilDebateTurn {
  round: number;
  response: string | null;
  error: string | null;
}

interface DebateUsageEstimate {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
}

const DEFAULT_MODEL = 'default';
const DEFAULT_JUDGE_MODEL = 'default';
const SHARED_REASONING_FILE = 'shared_reasoning.md';
const DEFAULT_SEATS: CouncilSeat[] = [
  {
    id: 0,
    mode: 'default',
    profileId: '',
    name: 'Analyst',
    model: DEFAULT_MODEL,
    brief: 'Evidence, assumptions, and risk.',
  },
  {
    id: 1,
    mode: 'default',
    profileId: '',
    name: 'Builder',
    model: DEFAULT_MODEL,
    brief: 'Practical implementation path.',
  },
  {
    id: 2,
    mode: 'default',
    profileId: '',
    name: 'Skeptic',
    model: DEFAULT_MODEL,
    brief: 'Failure modes and missing context.',
  },
];

const SEAT_COLORS = ['yellow', 'burgundy', 'navy', 'black', 'yellow'] as const;
const SEAT_FACES: MascotFace[] = ['thinking', 'writing', 'reading', 'curious', 'proud'];
const ACTIVE_SEAT_FACES: MascotFace[] = ['thinking', 'writing', 'thinking', 'reading', 'curious'];

const nextSeatId = (seats: CouncilSeat[]): number =>
  seats.reduce((max, seat) => Math.max(max, seat.id), -1) + 1;

function profileLabel(profile: AgentProfile): string {
  return profile.modelOverride ? `${profile.name} · ${profile.modelOverride}` : profile.name;
}

function profileModel(profile: AgentProfile | undefined): string {
  return profile?.modelOverride?.trim() || profile?.agentId?.trim() || profile?.id?.trim() || '';
}

function resolveSeat(seat: CouncilSeat, profiles: AgentProfile[], index: number): ResolvedSeat {
  const profile = profiles.find(p => p.id === seat.profileId);
  const fallbackName =
    seat.mode === 'profile' && profile ? profile.name : seat.name.trim() || `Juror ${index + 1}`;
  const fallbackModel = seat.mode === 'profile' ? profileModel(profile) : DEFAULT_MODEL;

  return {
    label: fallbackName,
    model: seat.model.trim() || fallbackModel,
    brief: seat.brief.trim(),
  };
}

function mascotColors(index: number) {
  const palette = getMascotPalette(SEAT_COLORS[index % SEAT_COLORS.length]);
  return {
    primaryColor: hexToArgbInt(palette.bodyFill),
    secondaryColor: hexToArgbInt(palette.neckShadowColor),
  };
}

function deliberationThought(
  seat: ResolvedSeat,
  index: number,
  t: (key: string) => string
): string {
  const brief = seat.brief.trim();
  if (brief) {
    return t('modelCouncil.thinkingWithBrief').replace('{brief}', brief);
  }

  const keys = [
    'modelCouncil.thought.evidence',
    'modelCouncil.thought.plan',
    'modelCouncil.thought.risk',
    'modelCouncil.thought.tradeoffs',
    'modelCouncil.thought.synthesis',
  ];
  return t(keys[index % keys.length]);
}

function buildCouncilQuestion(
  question: string,
  sharedReasoning: string,
  seats: ResolvedSeat[],
  judgeName: string
): string {
  const trimmedQuestion = question.trim();
  const trimmedSharedReasoning = sharedReasoning.trim();
  const roster = seats
    .map((seat, index) => {
      const brief = seat.brief ? ` — ${seat.brief}` : '';
      return `${index + 1}. ${seat.label} (${seat.model})${brief}`;
    })
    .join('\n');
  const commonPrefix = [
    `Council workspace: ${SHARED_REASONING_FILE}`,
    'Use this shared reasoning file as the common deliberation scratchpad.',
    '',
    'Council roster:',
    roster,
    '',
    `Judge agent: ${judgeName}`,
  ];

  if (!trimmedSharedReasoning) {
    return [...commonPrefix, '', 'User question:', trimmedQuestion].join('\n');
  }

  return [
    ...commonPrefix,
    '',
    `${SHARED_REASONING_FILE}:`,
    trimmedSharedReasoning,
    '',
    'User question:',
    trimmedQuestion,
  ].join('\n');
}

function buildDebateTurnQuestion(
  baseQuestion: string,
  seat: ResolvedSeat,
  round: number,
  totalRounds: number,
  transcript: CouncilDebateTurn[][],
  t: (key: string) => string
): string {
  const previousTurns = transcript
    .map((turns, seatIndex) => {
      if (turns.length === 0) return '';
      const body = turns
        .map(turn => {
          const text = turn.response || `[${turn.error || 'no response'}]`;
          return `Round ${turn.round}: ${text}`;
        })
        .join('\n');
      return `Juror ${seatIndex + 1} previous turns:\n${body}`;
    })
    .filter(Boolean)
    .join('\n\n');

  const phase =
    round === totalRounds
      ? t('modelCouncil.debateFinalInstruction')
      : t('modelCouncil.debateRoundInstruction');

  return [
    baseQuestion,
    '',
    `Debate round ${round} of ${totalRounds}.`,
    `You are ${seat.label}. Perspective: ${seat.brief || 'independent council juror'}.`,
    phase,
    previousTurns ? ['', 'Debate so far:', previousTurns].join('\n') : '',
    '',
    'Write this turn as a concise council thought plus your current conclusion.',
  ]
    .filter(Boolean)
    .join('\n');
}

function appendScratchpadRound(
  scratchpad: string,
  round: number,
  seats: ResolvedSeat[],
  roundResults: Array<{ index: number; turn: CouncilDebateTurn }>,
  t: (key: string) => string
): string {
  const existing = scratchpad.trim() || '# Shared reasoning';
  const lines = [
    '',
    '',
    `## ${t('modelCouncil.scratchpadRoundHeading').replace('{round}', String(round))}`,
  ];
  for (const { index, turn } of [...roundResults].sort((a, b) => a.index - b.index)) {
    const seat = seats[index];
    lines.push('', `### ${seat?.label || `Juror ${index + 1}`}`);
    if (turn.response) {
      lines.push(turn.response.trim());
    } else {
      lines.push(`_${t('modelCouncil.scratchpadNoResponse')}: ${turn.error || 'unknown'}_`);
    }
  }
  return `${existing}${lines.join('\n')}`;
}

function estimateTokens(text: string): number {
  return Math.max(1, Math.ceil(text.length / 4));
}

function formatTokenCount(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
}

function formatEstimatedCost(value: number): string {
  return new Intl.NumberFormat(undefined, {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 4,
    maximumFractionDigits: 4,
  }).format(value);
}

function buildMemberSynthesisInput(
  seat: ResolvedSeat,
  model: string,
  turns: CouncilDebateTurn[]
): CouncilMemberResult {
  const answeredTurns = turns.filter(turn => turn.response);
  if (answeredTurns.length === 0) {
    return {
      model,
      response: null,
      error: turns.find(turn => turn.error)?.error || 'no debate turns completed',
    };
  }

  return {
    model,
    response: [
      `${seat.label} debate record:`,
      ...turns.map(turn => {
        const text = turn.response || `[failed: ${turn.error || 'unknown'}]`;
        return `Round ${turn.round}: ${text}`;
      }),
    ].join('\n\n'),
    error: null,
  };
}

const ModelCouncilTab = () => {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const profiles = useAppSelector(selectAgentProfiles);
  const profileStatus = useAppSelector(state => state.agentProfiles.status as AgentProfilesStatus);

  const [question, setQuestion] = useState('');
  const [sharedReasoning, setSharedReasoning] = useState(
    [
      '# Shared reasoning',
      '- Claims the council agrees on:',
      '- Open disagreements:',
      '- Evidence or constraints to preserve:',
      '- Judge synthesis notes:',
    ].join('\n')
  );
  const [liveScratchpad, setLiveScratchpad] = useState<string | null>(null);
  const [juryCount, setJuryCount] = useState(3);
  const [debateRounds, setDebateRounds] = useState(3);
  const [seats, setSeats] = useState<CouncilSeat[]>(DEFAULT_SEATS);
  const [judgeMode, setJudgeMode] = useState<SeatMode>('default');
  const [judgeProfileId, setJudgeProfileId] = useState('');
  const [judgeName, setJudgeName] = useState('Chief Judge');
  const [judgeModel, setJudgeModel] = useState(DEFAULT_JUDGE_MODEL);
  const [running, setRunning] = useState(false);
  const [liveMembers, setLiveMembers] = useState<LiveMemberThought[]>([]);
  const [judgeSynthesizing, setJudgeSynthesizing] = useState(false);
  const [usageEstimate, setUsageEstimate] = useState<DebateUsageEstimate | null>(null);
  const [result, setResult] = useState<ModelCouncilResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (profileStatus === 'idle' && profiles.length === 0) {
      void dispatch(loadAgentProfiles());
    }
  }, [dispatch, profileStatus, profiles.length]);

  useEffect(() => {
    setSeats(prev => {
      if (prev.length === juryCount) return prev;
      if (prev.length > juryCount) return prev.slice(0, juryCount);

      const next = [...prev];
      while (next.length < juryCount) {
        const index = next.length;
        next.push({
          id: nextSeatId(next),
          mode: 'default',
          profileId: '',
          name: `${t('modelCouncil.jurorFallback')} ${index + 1}`,
          model: DEFAULT_MODEL,
          brief: '',
        });
      }
      return next;
    });
  }, [juryCount, t]);

  const judgeProfile = useMemo(
    () => profiles.find(profile => profile.id === judgeProfileId),
    [profiles, judgeProfileId]
  );

  const resolvedSeats = useMemo(
    () => seats.map((seat, index) => resolveSeat(seat, profiles, index)),
    [profiles, seats]
  );

  const resolvedJudgeModel =
    judgeModel.trim() ||
    (judgeMode === 'profile' ? profileModel(judgeProfile) : '') ||
    DEFAULT_JUDGE_MODEL;
  const resolvedJudgeName =
    judgeMode === 'profile' && judgeProfile ? judgeProfile.name : judgeName.trim() || 'Chief Judge';

  const canRun =
    !running &&
    question.trim().length > 0 &&
    resolvedSeats.some(seat => seat.model.trim().length > 0) &&
    resolvedJudgeModel.trim().length > 0;

  const updateSeat = useCallback((id: number, patch: Partial<CouncilSeat>) => {
    setSeats(prev => prev.map(seat => (seat.id === id ? { ...seat, ...patch } : seat)));
  }, []);

  const setSeatMode = useCallback(
    (seat: CouncilSeat, mode: SeatMode) => {
      updateSeat(seat.id, {
        mode,
        profileId: mode === 'profile' ? seat.profileId || profiles[0]?.id || '' : '',
        name: mode === 'custom' ? seat.name : seat.name || '',
        model: mode === 'profile' ? '' : seat.model || DEFAULT_MODEL,
      });
    },
    [profiles, updateSeat]
  );

  const handleRun = useCallback(async () => {
    if (running) return;
    const memberModels = resolvedSeats.map(seat => seat.model.trim()).filter(Boolean);
    const chairModel = resolvedJudgeModel.trim();
    if (question.trim().length === 0 || memberModels.length === 0 || chairModel.length === 0) {
      return;
    }
    setRunning(true);
    setJudgeSynthesizing(false);
    setLiveMembers(memberModels.map(() => ({ status: 'pending', member: null, turns: [] })));
    setLiveScratchpad(sharedReasoning);
    setUsageEstimate(null);
    setError(null);
    setResult(null);
    try {
      const transcript: CouncilDebateTurn[][] = memberModels.map(() => []);
      let currentScratchpad = sharedReasoning;
      let estimatedInputTokens = 0;
      let estimatedOutputTokens = 0;

      for (let round = 1; round <= debateRounds; round += 1) {
        setLiveMembers(prev => prev.map(entry => ({ ...entry, status: 'pending' })));

        const roundResults = await Promise.all(
          memberModels.map(async (model, index) => {
            const councilQuestion = buildCouncilQuestion(
              question,
              currentScratchpad,
              resolvedSeats,
              resolvedJudgeName
            );
            const turnQuestion = buildDebateTurnQuestion(
              councilQuestion,
              resolvedSeats[index],
              round,
              debateRounds,
              transcript,
              t
            );
            estimatedInputTokens += estimateTokens(turnQuestion);
            try {
              const member = await modelCouncilApi.answerMember({ question: turnQuestion, model });
              const turn: CouncilDebateTurn = {
                round,
                response: member.response,
                error: member.error,
              };
              estimatedOutputTokens += estimateTokens(member.response || member.error || '');
              setLiveMembers(prev =>
                prev.map((entry, entryIndex) =>
                  entryIndex === index
                    ? {
                        status: member.error ? 'failed' : 'answered',
                        member,
                        turns: [...entry.turns, turn],
                      }
                    : entry
                )
              );
              return { index, turn };
            } catch (memberError) {
              const errorText =
                memberError instanceof Error ? memberError.message : String(memberError);
              const failedMember: CouncilMemberResult = { model, response: null, error: errorText };
              const turn: CouncilDebateTurn = { round, response: null, error: errorText };
              estimatedOutputTokens += estimateTokens(errorText);
              setLiveMembers(prev =>
                prev.map((entry, entryIndex) =>
                  entryIndex === index
                    ? { status: 'failed', member: failedMember, turns: [...entry.turns, turn] }
                    : entry
                )
              );
              return { index, turn };
            }
          })
        );

        for (const { index, turn } of roundResults) {
          transcript[index].push(turn);
        }
        currentScratchpad = appendScratchpadRound(
          currentScratchpad,
          round,
          resolvedSeats,
          roundResults,
          t
        );
        setLiveScratchpad(currentScratchpad);
        setSharedReasoning(currentScratchpad);
      }

      const councilQuestion = buildCouncilQuestion(
        question,
        currentScratchpad,
        resolvedSeats,
        resolvedJudgeName
      );
      const memberResults = memberModels.map((model, index) =>
        buildMemberSynthesisInput(resolvedSeats[index], model, transcript[index])
      );
      const synthesisInputTokens = estimateTokens(
        `${councilQuestion}\n${JSON.stringify(memberResults)}`
      );
      estimatedInputTokens += synthesisInputTokens;
      setJudgeSynthesizing(true);
      const res = await modelCouncilApi.synthesizeCouncil({
        question: councilQuestion,
        members: memberResults,
        chair_model: chairModel,
      });
      estimatedOutputTokens += estimateTokens(res.synthesis);
      const totalTokens = estimatedInputTokens + estimatedOutputTokens;
      setUsageEstimate({
        inputTokens: estimatedInputTokens,
        outputTokens: estimatedOutputTokens,
        totalTokens,
        estimatedCostUsd: (totalTokens / 1000) * ESTIMATED_USD_PER_1K_TOKENS,
      });
      setResult(res);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setJudgeSynthesizing(false);
      setRunning(false);
      setLiveScratchpad(null);
    }
  }, [
    debateRounds,
    resolvedJudgeModel,
    resolvedJudgeName,
    resolvedSeats,
    question,
    running,
    sharedReasoning,
    t,
  ]);

  return (
    <div className="space-y-5">
      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
        <section className="space-y-3">
          <div>
            <h2 className="text-lg font-semibold text-stone-900 dark:text-neutral-50">
              {t('modelCouncil.title')}
            </h2>
            <p className="mt-1 max-w-3xl text-sm text-stone-600 dark:text-neutral-300">
              {t('modelCouncil.intro')}
            </p>
          </div>

          <div className="space-y-1.5">
            <label
              htmlFor="model-council-question"
              className="text-xs font-medium text-stone-600 dark:text-neutral-300">
              {t('modelCouncil.questionLabel')}
            </label>
            <textarea
              id="model-council-question"
              value={question}
              onChange={e => setQuestion(e.target.value)}
              rows={4}
              placeholder={t('modelCouncil.questionPlaceholder')}
              aria-label={t('modelCouncil.questionLabel')}
              className="w-full rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm text-stone-800 shadow-sm resize-y focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
            />
          </div>
        </section>

        <aside className="space-y-3 rounded-lg border border-stone-200 bg-white p-3 shadow-sm dark:border-neutral-800 dark:bg-neutral-900">
          <div>
            <p className="text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-neutral-400">
              {t('modelCouncil.settingsTitle')}
            </p>
            <p className="mt-1 text-xs text-stone-500 dark:text-neutral-400">
              {t('modelCouncil.settingsSummary')
                .replace('{count}', String(juryCount))
                .replace('{judge}', resolvedJudgeName)}
            </p>
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between gap-3">
              <label
                htmlFor="model-council-jury-count"
                className="text-xs font-medium text-stone-700 dark:text-neutral-200">
                {t('modelCouncil.juryCountLabel')}
              </label>
              <output className="rounded-md bg-stone-100 px-2 py-0.5 text-xs font-semibold text-stone-700 dark:bg-neutral-800 dark:text-neutral-200">
                {juryCount}
              </output>
            </div>
            <input
              id="model-council-jury-count"
              type="range"
              min={MIN_MEMBERS}
              max={MAX_MEMBERS}
              value={juryCount}
              aria-label={t('modelCouncil.juryCountLabel')}
              onChange={e => setJuryCount(Number(e.target.value))}
              className="w-full accent-primary-500"
            />
            <div className="grid grid-cols-5 gap-1">
              {Array.from({ length: MAX_MEMBERS }, (_, index) => index + 1).map(count => (
                <button
                  key={count}
                  type="button"
                  onClick={() => setJuryCount(count)}
                  aria-pressed={juryCount === count}
                  className={`rounded-md border px-2 py-1 text-xs font-medium ${
                    juryCount === count
                      ? 'border-primary-500 bg-primary-50 text-primary-700 dark:bg-primary-500/15 dark:text-primary-200'
                      : 'border-stone-200 text-stone-500 hover:bg-stone-50 dark:border-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800'
                  }`}>
                  {count}
                </button>
              ))}
            </div>
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between gap-3">
              <label
                htmlFor="model-council-debate-rounds"
                className="text-xs font-medium text-stone-700 dark:text-neutral-200">
                {t('modelCouncil.debateRoundsLabel')}
              </label>
              <output className="rounded-md bg-stone-100 px-2 py-0.5 text-xs font-semibold text-stone-700 dark:bg-neutral-800 dark:text-neutral-200">
                {debateRounds}
              </output>
            </div>
            <input
              id="model-council-debate-rounds"
              type="range"
              min={MIN_DEBATE_ROUNDS}
              max={MAX_DEBATE_ROUNDS}
              value={debateRounds}
              aria-label={t('modelCouncil.debateRoundsLabel')}
              onChange={e => setDebateRounds(Number(e.target.value))}
              className="w-full accent-primary-500"
            />
            <div className="grid grid-cols-3 gap-1">
              {Array.from(
                { length: MAX_DEBATE_ROUNDS - MIN_DEBATE_ROUNDS + 1 },
                (_, index) => index + MIN_DEBATE_ROUNDS
              ).map(rounds => (
                <button
                  key={rounds}
                  type="button"
                  onClick={() => setDebateRounds(rounds)}
                  aria-pressed={debateRounds === rounds}
                  className={`rounded-md border px-2 py-1 text-xs font-medium ${
                    debateRounds === rounds
                      ? 'border-primary-500 bg-primary-50 text-primary-700 dark:bg-primary-500/15 dark:text-primary-200'
                      : 'border-stone-200 text-stone-500 hover:bg-stone-50 dark:border-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800'
                  }`}>
                  {rounds}
                </button>
              ))}
            </div>
            <p className="text-[11px] leading-4 text-stone-500 dark:text-neutral-400">
              {t('modelCouncil.debateRoundsHelp')}
            </p>
          </div>

          <div className="space-y-2">
            <label
              htmlFor="model-council-judge-mode"
              className="text-xs font-medium text-stone-700 dark:text-neutral-200">
              {t('modelCouncil.judgeAgentLabel')}
            </label>
            <select
              id="model-council-judge-mode"
              value={judgeMode}
              onChange={e => {
                const mode = e.target.value as SeatMode;
                setJudgeMode(mode);
                setJudgeModel(mode === 'default' ? DEFAULT_JUDGE_MODEL : '');
              }}
              className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-800 focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100">
              <option value="default">{t('modelCouncil.defaultJudge')}</option>
              <option value="profile">{t('modelCouncil.savedProfile')}</option>
              <option value="custom">{t('modelCouncil.customAgent')}</option>
            </select>

            {judgeMode === 'profile' && (
              <select
                value={judgeProfileId}
                aria-label={t('modelCouncil.judgeProfileLabel')}
                onChange={e => {
                  setJudgeProfileId(e.target.value);
                  setJudgeModel('');
                }}
                className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-800 focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100">
                <option value="">{t('modelCouncil.chooseProfile')}</option>
                {profiles.map(profile => (
                  <option key={profile.id} value={profile.id}>
                    {profileLabel(profile)}
                  </option>
                ))}
              </select>
            )}

            {judgeMode === 'custom' && (
              <input
                type="text"
                value={judgeName}
                onChange={e => setJudgeName(e.target.value)}
                aria-label={t('modelCouncil.judgeNameLabel')}
                placeholder={t('modelCouncil.judgeNamePlaceholder')}
                className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-800 focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
              />
            )}

            <input
              type="text"
              value={judgeModel}
              onChange={e => setJudgeModel(e.target.value)}
              aria-label={t('modelCouncil.chairLabel')}
              placeholder={t('modelCouncil.chairPlaceholder')}
              className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 font-mono text-sm text-stone-800 focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
            />
          </div>
        </aside>
      </div>

      <section
        aria-labelledby="model-council-shared-reasoning-heading"
        className="grid gap-3 rounded-lg border border-stone-200 bg-white p-3 shadow-sm dark:border-neutral-800 dark:bg-neutral-900 lg:grid-cols-[220px_minmax(0,1fr)]">
        <div>
          <p
            id="model-council-shared-reasoning-heading"
            className="font-mono text-sm font-semibold text-stone-900 dark:text-neutral-50">
            {SHARED_REASONING_FILE}
          </p>
          <p className="mt-1 text-xs text-stone-500 dark:text-neutral-400">
            {t('modelCouncil.sharedReasoningHelp')}
          </p>
        </div>
        <textarea
          value={sharedReasoning}
          onChange={e => setSharedReasoning(e.target.value)}
          rows={5}
          aria-label={t('modelCouncil.sharedReasoningLabel')}
          className="min-h-[132px] w-full rounded-lg border border-stone-200 bg-stone-50 px-3 py-2 font-mono text-xs text-stone-800 resize-y focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
        />
      </section>

      <section aria-labelledby="model-council-roster-heading" className="space-y-3">
        <div className="flex flex-wrap items-end justify-between gap-2">
          <div>
            <h3
              id="model-council-roster-heading"
              className="text-sm font-semibold text-stone-800 dark:text-neutral-100">
              {t('modelCouncil.rosterHeading')}
            </h3>
            <p className="text-xs text-stone-500 dark:text-neutral-400">
              {t('modelCouncil.rosterHelp')}
            </p>
          </div>
          {profileStatus === 'loading' && (
            <span className="text-xs text-stone-500 dark:text-neutral-400">
              {t('modelCouncil.loadingProfiles')}
            </span>
          )}
        </div>

        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {seats.map((seat, index) => {
            const resolved = resolvedSeats[index];
            const colors = mascotColors(index);
            const activeFace = running
              ? ACTIVE_SEAT_FACES[index % ACTIVE_SEAT_FACES.length]
              : SEAT_FACES[index % SEAT_FACES.length];
            return (
              <article
                key={seat.id}
                className={`rounded-lg border bg-white p-3 shadow-sm transition-colors dark:bg-neutral-900 ${
                  running
                    ? 'border-primary-300 ring-2 ring-primary-100 dark:border-primary-500/50 dark:ring-primary-500/10'
                    : 'border-stone-200 dark:border-neutral-800'
                }`}>
                <div className="flex gap-3">
                  <div
                    className={`h-20 w-20 shrink-0 overflow-hidden rounded-lg bg-stone-100 dark:bg-neutral-800 ${
                      running ? 'animate-pulse' : ''
                    }`}>
                    <RiveMascot
                      size="100%"
                      face={activeFace}
                      primaryColor={colors.primaryColor}
                      secondaryColor={colors.secondaryColor}
                    />
                  </div>
                  <div className="min-w-0 flex-1 space-y-2">
                    <div className="flex items-center justify-between gap-2">
                      <p className="truncate text-sm font-semibold text-stone-900 dark:text-neutral-50">
                        {resolved.label}
                      </p>
                      <span className="rounded-md bg-stone-100 px-1.5 py-0.5 text-[10px] font-semibold uppercase text-stone-500 dark:bg-neutral-800 dark:text-neutral-400">
                        {t('modelCouncil.jurorLabel').replace('{n}', String(index + 1))}
                      </span>
                    </div>

                    <div
                      role="tablist"
                      aria-label={t('modelCouncil.profileModeLabel')}
                      className="grid grid-cols-3 gap-1">
                      {(['default', 'profile', 'custom'] as SeatMode[]).map(mode => (
                        <button
                          key={mode}
                          type="button"
                          role="tab"
                          aria-selected={seat.mode === mode}
                          onClick={() => setSeatMode(seat, mode)}
                          className={`rounded-md px-2 py-1 text-[11px] font-medium ${
                            seat.mode === mode
                              ? 'bg-primary-500 text-white'
                              : 'bg-stone-100 text-stone-600 hover:bg-stone-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700'
                          }`}>
                          {t(`modelCouncil.mode.${mode}`)}
                        </button>
                      ))}
                    </div>
                  </div>
                </div>

                <div className="mt-3 space-y-2">
                  {seat.mode === 'profile' ? (
                    <select
                      value={seat.profileId}
                      aria-label={t('modelCouncil.memberProfileAria').replace(
                        '{n}',
                        String(index + 1)
                      )}
                      onChange={e => updateSeat(seat.id, { profileId: e.target.value, model: '' })}
                      className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-800 focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100">
                      <option value="">{t('modelCouncil.chooseProfile')}</option>
                      {profiles.map(profile => (
                        <option key={profile.id} value={profile.id}>
                          {profileLabel(profile)}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <input
                      type="text"
                      value={seat.name}
                      onChange={e => updateSeat(seat.id, { name: e.target.value })}
                      aria-label={t('modelCouncil.memberNameAria').replace(
                        '{n}',
                        String(index + 1)
                      )}
                      placeholder={t('modelCouncil.memberNamePlaceholder')}
                      className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-sm text-stone-800 focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
                    />
                  )}

                  <input
                    type="text"
                    value={seat.model}
                    onChange={e => updateSeat(seat.id, { model: e.target.value })}
                    aria-label={t('modelCouncil.memberAria').replace('{n}', String(index + 1))}
                    placeholder={
                      seat.mode === 'profile'
                        ? t('modelCouncil.profileModelPlaceholder')
                        : t('modelCouncil.memberPlaceholder')
                    }
                    className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 font-mono text-sm text-stone-800 focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
                  />

                  <textarea
                    value={seat.brief}
                    onChange={e => updateSeat(seat.id, { brief: e.target.value })}
                    rows={2}
                    aria-label={t('modelCouncil.memberBriefAria').replace('{n}', String(index + 1))}
                    placeholder={t('modelCouncil.memberBriefPlaceholder')}
                    className="w-full rounded-lg border border-stone-200 bg-white px-3 py-1.5 text-xs text-stone-700 resize-none focus:outline-none focus:ring-2 focus:ring-primary-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
                  />
                </div>
              </article>
            );
          })}
        </div>
      </section>

      {running && (
        <section
          aria-labelledby="model-council-deliberation-heading"
          className="space-y-3 rounded-lg border border-primary-200 bg-primary-50/60 p-3 dark:border-primary-500/30 dark:bg-primary-500/10">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <h3
                id="model-council-deliberation-heading"
                className="text-sm font-semibold text-stone-900 dark:text-neutral-50">
                {t('modelCouncil.deliberationHeading')}
              </h3>
              <p className="text-xs text-stone-600 dark:text-neutral-300">
                {t('modelCouncil.deliberationHelp')}
              </p>
            </div>
            <span
              role="status"
              aria-live="polite"
              className="rounded-md bg-white px-2 py-1 text-xs font-medium text-primary-700 shadow-sm dark:bg-neutral-950 dark:text-primary-200">
              {t('modelCouncil.runningHint')}
            </span>
          </div>

          <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
            {resolvedSeats.map((seat, index) => {
              const colors = mascotColors(index);
              const liveMember = liveMembers[index];
              const answered = liveMember?.status === 'answered';
              const failed = liveMember?.status === 'failed';
              const turns = liveMember?.turns ?? [];
              const hasTurns = turns.length > 0;
              const waitingText = deliberationThought(seat, index, t);
              return (
                <div
                  key={`${seat.label}-${index}`}
                  className={`rounded-lg border bg-white/90 p-3 shadow-sm dark:bg-neutral-950/80 ${
                    failed
                      ? 'border-coral-200 dark:border-coral-500/30'
                      : answered
                        ? 'border-sage-200 dark:border-sage-500/30'
                        : 'border-white/80 dark:border-neutral-800'
                  }`}>
                  <div className="flex items-start gap-3">
                    <div
                      className={`h-14 w-14 shrink-0 overflow-hidden rounded-lg bg-stone-100 dark:bg-neutral-800 ${
                        liveMember?.status === 'pending' || !liveMember ? 'animate-pulse' : ''
                      }`}>
                      <RiveMascot
                        size="100%"
                        face={
                          failed
                            ? 'curious'
                            : answered
                              ? 'proud'
                              : ACTIVE_SEAT_FACES[index % ACTIVE_SEAT_FACES.length]
                        }
                        primaryColor={colors.primaryColor}
                        secondaryColor={colors.secondaryColor}
                      />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-2">
                        <p className="truncate text-sm font-semibold text-stone-900 dark:text-neutral-50">
                          {seat.label}
                        </p>
                        <span
                          className={`shrink-0 rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase ${
                            failed
                              ? 'bg-coral-100 text-coral-700 dark:bg-coral-500/20 dark:text-coral-300'
                              : answered
                                ? 'bg-sage-100 text-sage-700 dark:bg-sage-500/20 dark:text-sage-300'
                                : 'bg-primary-100 text-primary-700 dark:bg-primary-500/20 dark:text-primary-200'
                          }`}>
                          {failed
                            ? t('modelCouncil.memberFailed')
                            : answered
                              ? t('modelCouncil.memberAnswered')
                              : t('modelCouncil.thinkingBadge')}
                        </span>
                      </div>
                      <div className="mt-2 max-h-52 space-y-1.5 overflow-y-auto pr-1">
                        {hasTurns ? (
                          turns.map(turn => (
                            <div
                              key={`${seat.label}-${index}-${turn.round}`}
                              className={`rounded-md border px-2 py-1.5 ${
                                turn.error
                                  ? 'border-coral-100 bg-coral-50/70 dark:border-coral-500/20 dark:bg-coral-500/10'
                                  : 'border-stone-200 bg-stone-50 dark:border-neutral-800 dark:bg-neutral-900'
                              }`}>
                              <p className="text-[10px] font-semibold uppercase text-stone-500 dark:text-neutral-400">
                                {t('modelCouncil.roundLabel').replace(
                                  '{round}',
                                  String(turn.round)
                                )}
                              </p>
                              {turn.error ? (
                                <p className="mt-0.5 line-clamp-4 whitespace-pre-wrap text-xs text-coral-600 dark:text-coral-300">
                                  {turn.error}
                                </p>
                              ) : (
                                <div className="mt-0.5 text-stone-600 dark:text-neutral-300 [&_.prose]:text-xs [&_.prose]:leading-5 [&_.prose_p]:my-0">
                                  <BubbleMarkdown content={turn.response || ''} />
                                </div>
                              )}
                            </div>
                          ))
                        ) : (
                          <p className="line-clamp-5 whitespace-pre-wrap text-xs text-stone-600 dark:text-neutral-300">
                            {waitingText}
                          </p>
                        )}
                        {liveMember?.status === 'pending' && hasTurns && (
                          <p className="text-[11px] text-primary-700 dark:text-primary-200">
                            {t('modelCouncil.currentRoundThinking')}
                          </p>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}

            <div className="rounded-lg border border-primary-200 bg-white p-3 shadow-sm dark:border-primary-500/30 dark:bg-neutral-950">
              <div className="flex items-start gap-3">
                <div className="h-14 w-14 shrink-0 overflow-hidden rounded-lg bg-stone-100 dark:bg-neutral-800">
                  <RiveMascot size="100%" face="reading" />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-2">
                    <p className="truncate text-sm font-semibold text-stone-900 dark:text-neutral-50">
                      {resolvedJudgeName}
                    </p>
                    <span className="shrink-0 rounded bg-amber-100 px-1.5 py-0.5 text-[9px] font-semibold uppercase text-amber-700 dark:bg-amber-500/20 dark:text-amber-200">
                      {judgeSynthesizing
                        ? t('modelCouncil.judgeSynthesizingBadge')
                        : t('modelCouncil.judgeWaitingBadge')}
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-stone-600 dark:text-neutral-300">
                    {judgeSynthesizing
                      ? t('modelCouncil.judgeSynthesizingThought')
                      : t('modelCouncil.judgeWaitingThought')}
                  </p>
                </div>
              </div>
            </div>
          </div>

          <div className="rounded-lg border border-stone-200 bg-white p-3 shadow-sm dark:border-neutral-800 dark:bg-neutral-950">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <h4 className="font-mono text-sm font-semibold text-stone-900 dark:text-neutral-50">
                  {SHARED_REASONING_FILE}
                </h4>
                <p className="text-xs text-stone-500 dark:text-neutral-400">
                  {t('modelCouncil.liveScratchpadHelp')}
                </p>
              </div>
              <span className="rounded bg-primary-100 px-1.5 py-0.5 text-[9px] font-semibold uppercase text-primary-700 dark:bg-primary-500/20 dark:text-primary-200">
                {t('modelCouncil.liveScratchpadBadge')}
              </span>
            </div>
            <div className="mt-3 max-h-72 overflow-y-auto rounded-md border border-stone-200 bg-stone-50 px-3 py-2 text-stone-700 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-200 [&_.prose]:text-xs [&_.prose]:leading-5">
              <BubbleMarkdown content={liveScratchpad || sharedReasoning} />
            </div>
          </div>
        </section>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => void handleRun()}
          disabled={!canRun}
          className="rounded-lg bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:opacity-50">
          {running ? t('modelCouncil.running') : t('modelCouncil.run')}
        </button>
        {running && (
          <span
            role="status"
            aria-live="polite"
            className="text-xs text-stone-500 dark:text-neutral-400">
            {t('modelCouncil.runningHint')}
          </span>
        )}
      </div>

      {error && (
        <p role="alert" className="text-xs text-coral-700 dark:text-coral-300">
          {t('modelCouncil.errorPrefix')} {error}
        </p>
      )}

      {result && (
        <section aria-labelledby="model-council-results-heading" className="space-y-3 pt-1">
          <h3
            id="model-council-results-heading"
            className="text-sm font-semibold text-stone-800 dark:text-neutral-100">
            {t('modelCouncil.resultsHeading')}
          </h3>

          <div className="grid gap-2 sm:grid-cols-2">
            {result.members.map((member, index) => (
              <div
                key={`${member.model}-${index}`}
                className="rounded-lg border border-stone-200 p-3 space-y-1.5 dark:border-neutral-800">
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate font-mono text-xs font-medium text-stone-700 dark:text-neutral-200">
                    {member.model}
                  </span>
                  <span
                    className={`inline-flex shrink-0 items-center rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide ${
                      member.error
                        ? 'bg-coral-100 text-coral-700 dark:bg-coral-500/20 dark:text-coral-300'
                        : 'bg-sage-100 text-sage-700 dark:bg-sage-500/20 dark:text-sage-300'
                    }`}>
                    {member.error
                      ? t('modelCouncil.memberFailed')
                      : t('modelCouncil.memberAnswered')}
                  </span>
                </div>
                {member.error ? (
                  <p className="text-xs text-coral-600 dark:text-coral-400">{member.error}</p>
                ) : (
                  <div className="break-words text-stone-600 dark:text-neutral-300 [&_.prose]:text-xs [&_.prose]:leading-5 [&_.prose_p]:my-0">
                    <BubbleMarkdown content={member.response || ''} />
                  </div>
                )}
              </div>
            ))}
          </div>

          <div className="rounded-lg border border-primary-200 bg-primary-50 p-3 space-y-1 dark:border-primary-500/30 dark:bg-primary-500/10">
            <div className="flex items-center justify-between gap-2">
              <h4 className="text-xs font-semibold text-stone-800 dark:text-neutral-100">
                {t('modelCouncil.synthesisHeading')}
              </h4>
              <span className="truncate font-mono text-[10px] text-stone-500 dark:text-neutral-400">
                {t('modelCouncil.synthesisBy').replace('{model}', result.chair_model)}
              </span>
            </div>
            <div className="break-words text-stone-700 dark:text-neutral-200 [&_.prose]:text-sm [&_.prose]:leading-6">
              <BubbleMarkdown content={result.synthesis} />
            </div>
          </div>

          {usageEstimate && (
            <div className="rounded-lg border border-stone-200 bg-white p-3 shadow-sm dark:border-neutral-800 dark:bg-neutral-900">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <h4 className="text-xs font-semibold text-stone-800 dark:text-neutral-100">
                    {t('modelCouncil.usageHeading')}
                  </h4>
                  <p className="text-[11px] text-stone-500 dark:text-neutral-400">
                    {t('modelCouncil.usageEstimated')}
                  </p>
                </div>
                <span className="rounded-md bg-stone-100 px-2 py-1 font-mono text-xs font-semibold text-stone-700 dark:bg-neutral-800 dark:text-neutral-200">
                  {formatEstimatedCost(usageEstimate.estimatedCostUsd)}
                </span>
              </div>
              <dl className="mt-3 grid gap-2 sm:grid-cols-3">
                <div className="rounded-md bg-stone-50 px-2 py-1.5 dark:bg-neutral-950">
                  <dt className="text-[10px] uppercase text-stone-500 dark:text-neutral-400">
                    {t('modelCouncil.usageInputTokens')}
                  </dt>
                  <dd className="font-mono text-sm font-semibold text-stone-800 dark:text-neutral-100">
                    {formatTokenCount(usageEstimate.inputTokens)}
                  </dd>
                </div>
                <div className="rounded-md bg-stone-50 px-2 py-1.5 dark:bg-neutral-950">
                  <dt className="text-[10px] uppercase text-stone-500 dark:text-neutral-400">
                    {t('modelCouncil.usageOutputTokens')}
                  </dt>
                  <dd className="font-mono text-sm font-semibold text-stone-800 dark:text-neutral-100">
                    {formatTokenCount(usageEstimate.outputTokens)}
                  </dd>
                </div>
                <div className="rounded-md bg-stone-50 px-2 py-1.5 dark:bg-neutral-950">
                  <dt className="text-[10px] uppercase text-stone-500 dark:text-neutral-400">
                    {t('modelCouncil.usageTotalTokens')}
                  </dt>
                  <dd className="font-mono text-sm font-semibold text-stone-800 dark:text-neutral-100">
                    {formatTokenCount(usageEstimate.totalTokens)}
                  </dd>
                </div>
              </dl>
            </div>
          )}
        </section>
      )}
    </div>
  );
};

export default ModelCouncilTab;
