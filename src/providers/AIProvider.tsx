import {
  createContext,
  useContext,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { useAppDispatch, useAppSelector } from "../store/hooks";
import {
  setAIStatus,
  setAIError,
  setMemoryInitialized,
  setLoadedSkillsCount,
} from "../store/aiSlice";
import { MemoryManager } from "../lib/ai/memory/manager";
import { SessionManager } from "../lib/ai/sessions/manager";
import { SkillRegistry } from "../lib/ai/skills/registry";
import { ToolRegistry } from "../lib/ai/tools/registry";
import { EntityManager } from "../lib/ai/entities/manager";
import { CustomLLMProvider } from "../lib/ai/providers/custom";
import { OpenAIEmbeddingProvider } from "../lib/ai/providers/openai";
import { NullEmbeddingProvider } from "../lib/ai/providers/embeddings";
import { loadConstitution } from "../lib/ai/constitution/loader";
import { createMemorySearchTool } from "../lib/ai/tools/memory-search";
import { createMemoryReadTool } from "../lib/ai/tools/memory-read";
import { createMemoryWriteTool } from "../lib/ai/tools/memory-write";
import { createWebSearchTool } from "../lib/ai/tools/web-search";
import type { ConstitutionConfig } from "../lib/ai/constitution/types";
import type { LLMProvider } from "../lib/ai/providers/interface";
import type { EmbeddingProvider } from "../lib/ai/providers/embeddings";

/** AI context value */
interface AIContextValue {
  memoryManager: MemoryManager;
  sessionManager: SessionManager;
  skillRegistry: SkillRegistry;
  toolRegistry: ToolRegistry;
  entityManager: EntityManager;
  llmProvider: LLMProvider | null;
  embeddingProvider: EmbeddingProvider;
  constitution: ConstitutionConfig | null;
  isReady: boolean;
}

const AIContext = createContext<AIContextValue | null>(null);

export function useAI(): AIContextValue {
  const ctx = useContext(AIContext);
  if (!ctx) {
    throw new Error("useAI must be used within an AIProvider");
  }
  return ctx;
}

export default function AIProvider({ children }: { children: ReactNode }) {
  const dispatch = useAppDispatch();
  const { config } = useAppSelector((state) => state.ai);
  const { token } = useAppSelector((state) => state.auth);

  const memoryManagerRef = useRef(new MemoryManager());
  const sessionManagerRef = useRef(new SessionManager());
  const skillRegistryRef = useRef(new SkillRegistry());
  const toolRegistryRef = useRef(new ToolRegistry());
  const entityManagerRef = useRef(new EntityManager());
  const constitutionRef = useRef<ConstitutionConfig | null>(null);
  const llmProviderRef = useRef<LLMProvider | null>(null);
  const embeddingProviderRef = useRef<EmbeddingProvider>(
    new NullEmbeddingProvider(),
  );
  const isReadyRef = useRef(false);

  useEffect(() => {
    if (!token) return;

    let cancelled = false;

    async function initAI() {
      dispatch(setAIStatus("initializing"));

      try {
        // 1. Load constitution
        const constitution = await loadConstitution();
        if (cancelled) return;
        constitutionRef.current = constitution;

        // 2. Initialize memory system
        await memoryManagerRef.current.init();
        if (cancelled) return;
        dispatch(setMemoryInitialized(true));

        // 3. Initialize entity database
        await entityManagerRef.current.init();
        if (cancelled) return;

        // 4. Setup embedding provider
        if (config.openaiApiKey) {
          const provider = new OpenAIEmbeddingProvider({
            id: "openai",
            apiKey: config.openaiApiKey,
          });
          embeddingProviderRef.current = provider;
          memoryManagerRef.current.setEmbeddingProvider(provider);
        }

        // 5. Setup LLM provider
        if (config.llmEndpoint) {
          llmProviderRef.current = new CustomLLMProvider({
            id: "custom",
            endpoint: config.llmEndpoint,
            model: config.llmModel,
          });
        }

        // 6. Index memory files
        await memoryManagerRef.current.indexAll();
        if (cancelled) return;

        // 7. Initialize sessions
        await sessionManagerRef.current.init();
        if (cancelled) return;

        // 8. Register tools
        const toolReg = toolRegistryRef.current;
        toolReg.register(
          createMemorySearchTool(memoryManagerRef.current),
        );
        toolReg.register(
          createMemoryReadTool(memoryManagerRef.current),
        );
        toolReg.register(
          createMemoryWriteTool(
            memoryManagerRef.current,
            constitution,
          ),
        );
        toolReg.register(
          createWebSearchTool({
            endpoint: config.webSearchEndpoint,
            apiKey: config.webSearchApiKey,
          }),
        );

        // 9. Load skills (with lifecycle hooks)
        const skillReg = skillRegistryRef.current;
        skillReg.setManagers({
          memory: memoryManagerRef.current,
          session: sessionManagerRef.current,
          tools: toolReg,
          entities: entityManagerRef.current,
        });
        await skillReg.reload();
        if (cancelled) return;
        dispatch(setLoadedSkillsCount(skillReg.count));

        isReadyRef.current = true;
        dispatch(setAIStatus("ready"));
      } catch (error) {
        if (!cancelled) {
          const msg =
            error instanceof Error ? error.message : String(error);
          dispatch(setAIError(msg));
        }
      }
    }

    initAI();

    return () => {
      cancelled = true;
      // Unload skill hooks on cleanup
      skillRegistryRef.current.unloadAll().catch(console.error);
    };
  }, [token, config, dispatch]);

  const contextValue: AIContextValue = {
    memoryManager: memoryManagerRef.current,
    sessionManager: sessionManagerRef.current,
    skillRegistry: skillRegistryRef.current,
    toolRegistry: toolRegistryRef.current,
    entityManager: entityManagerRef.current,
    llmProvider: llmProviderRef.current,
    embeddingProvider: embeddingProviderRef.current,
    constitution: constitutionRef.current,
    isReady: isReadyRef.current,
  };

  return (
    <AIContext.Provider value={contextValue}>{children}</AIContext.Provider>
  );
}
