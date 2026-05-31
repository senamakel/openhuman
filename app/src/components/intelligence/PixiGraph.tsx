/**
 * Thin React host for the imperative Pixi + d3-force renderer.
 *
 * Mounts the WebGL graph into a div and forwards hover/open back to the
 * parent `MemoryGraph` chrome (footer + preview). Pixi owns all canvas
 * interaction; React only manages the lifecycle. Callbacks are held in
 * refs so changing them never tears down and re-creates the GPU context.
 */
import { useEffect, useRef } from 'react';

import { type GraphEdge, type GraphMode, type GraphNode } from '../../utils/tauriCommands';
import { buildGraph } from './memoryGraphLayout';
import { mountPixiGraph, type PixiGraphHandle } from './pixiGraphRenderer';

interface PixiGraphProps {
  nodes: GraphNode[];
  edges: GraphEdge[];
  mode: GraphMode;
  dark: boolean;
  /** Bump to recentre the view (Reset view button). */
  resetSignal: number;
  onHover: (node: GraphNode | null) => void;
  onOpen: (node: GraphNode) => void;
}

export function PixiGraph({
  nodes,
  edges,
  mode,
  dark,
  resetSignal,
  onHover,
  onOpen,
}: PixiGraphProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const handleRef = useRef<PixiGraphHandle | null>(null);
  const onHoverRef = useRef(onHover);
  const onOpenRef = useRef(onOpen);
  const darkRef = useRef(dark);
  onHoverRef.current = onHover;
  onOpenRef.current = onOpen;
  darkRef.current = dark;

  // (Re)mount the renderer whenever the graph data or mode changes.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let cancelled = false;
    const { simNodes, links } = buildGraph(nodes, edges, mode);
    const pending = mountPixiGraph(host, {
      simNodes,
      links,
      dark: darkRef.current,
      onHover: n => onHoverRef.current(n),
      onOpen: n => onOpenRef.current(n),
    })
      .then(handle => {
        if (cancelled) {
          handle.destroy();
          return null;
        }
        handleRef.current = handle;
        return handle;
      })
      .catch(err => {
        console.error('[memory-graph] Pixi init failed', err);
        return null;
      });
    return () => {
      cancelled = true;
      handleRef.current = null;
      void pending.then(handle => handle?.destroy());
    };
  }, [nodes, edges, mode]);

  useEffect(() => {
    handleRef.current?.setTheme(dark);
  }, [dark]);

  useEffect(() => {
    if (resetSignal > 0) handleRef.current?.resetView();
  }, [resetSignal]);

  return (
    <div
      ref={hostRef}
      data-testid="memory-graph-canvas"
      className="block w-full"
      style={{ height: 'min(640px, calc(100vh - 22rem))' }}
    />
  );
}
