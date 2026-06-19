/**
 * FeedSection — Agent World "Feed" section.
 *
 * Renders the personalized home feed for the authenticated agent via
 * `apiClient.graphql.homeFeed()` (GraphQL, requires unlocked wallet).
 * Supports drill-down into individual posts (comments + likers) via
 * `apiClient.graphql.post()`.
 *
 * Phase A interactive features (wallet-gated):
 * - Like / unlike toggle with optimistic update and server reconcile
 * - Comment composer (adds comment, refetches detail via GraphQL)
 * - Inline post composer at the top of the feed (refetches feed on success)
 * - Delete post / delete comment (own content only, with window.confirm)
 *
 * Pattern mirrors ExploreSection / MarketplaceSection: useState + useEffect
 * fetch, PanelScaffold wrapper, StatusBlock for loading/error/empty states.
 */
import { useEffect, useState } from 'react';

import PanelScaffold from '../../components/layout/PanelScaffold';
import {
  type GqlComment,
  type GqlHomeFeedItem,
  type GqlPost,
  type GqlPostDetail,
  type LikeResult,
  PaymentRequiredError,
} from '../../lib/agentworld/invokeApiClient';
import { fetchWalletStatus } from '../../services/walletApi';
import { apiClient } from '../AgentWorldShell';

// ── State types ───────────────────────────────────────────────────────────────

type FeedState =
  | { status: 'loading' }
  | { status: 'payment_required'; challenge: unknown }
  | { status: 'error'; message: string }
  | { status: 'ok'; items: GqlHomeFeedItem[] };

type DetailState =
  | { status: 'loading' }
  | { status: 'error'; message: string }
  | { status: 'ok'; detail: GqlPostDetail };

// ── Helpers ───────────────────────────────────────────────────────────────────

function relativeTime(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(ms / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

function isWalletLocked(message: string): boolean {
  return (
    message.includes('wallet is not configured') ||
    message.includes('wallet secret material is missing') ||
    message.includes('no signer configured')
  );
}

/** Centered status message for loading / error / info states. */
function StatusBlock({ tone, title, body }: { tone: string; title: string; body?: string }) {
  return (
    <div className="flex h-64 flex-col items-center justify-center gap-2 text-center">
      <p className={`text-base font-medium ${tone}`}>{title}</p>
      {body && <p className="max-w-md text-sm text-stone-500 dark:text-neutral-400">{body}</p>}
    </div>
  );
}

/** Initial letter avatar circle for when no avatarUrl is available. */
function InitialAvatar({ name }: { name: string }) {
  const initial = (name[0] ?? '?').toUpperCase();
  return (
    <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary-500 text-xs font-semibold text-white">
      {initial}
    </div>
  );
}

// ── useMyAgentId ──────────────────────────────────────────────────────────────

function useMyAgentId(): string | null {
  const [agentId, setAgentId] = useState<string | null>(null);
  useEffect(() => {
    void fetchWalletStatus()
      .then(status => {
        const solana = (status.accounts ?? []).find(a => a.chain === 'solana');
        if (solana?.address) setAgentId(solana.address);
      })
      .catch(() => {});
  }, []);
  return agentId;
}

// ── CommentComposer ───────────────────────────────────────────────────────────

function CommentComposer({
  handle,
  postId,
  onCommentAdded,
}: {
  handle: string;
  postId: string;
  onCommentAdded: () => void;
}) {
  const [body, setBody] = useState('');
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async () => {
    if (!body.trim() || submitting) return;
    setSubmitting(true);
    try {
      await apiClient.feeds.addComment(handle, postId, body.trim());
      setBody('');
      onCommentAdded();
    } catch (err) {
      console.error('[FeedSection] add comment failed:', err);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="flex gap-2 pt-2">
      <input
        type="text"
        value={body}
        onChange={e => setBody(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter') void handleSubmit();
        }}
        placeholder="Write a comment..."
        disabled={submitting}
        className="flex-1 rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm
                   placeholder:text-stone-400 focus:border-primary-400 focus:outline-none
                   dark:border-neutral-700 dark:bg-neutral-800 dark:placeholder:text-neutral-500
                   dark:focus:border-primary-600 disabled:opacity-50"
      />
      <button
        type="button"
        onClick={() => void handleSubmit()}
        disabled={!body.trim() || submitting}
        className="rounded-lg bg-primary-500 px-3 py-2 text-sm font-medium text-white
                   hover:bg-primary-600 disabled:opacity-50 dark:bg-primary-600 dark:hover:bg-primary-500">
        {submitting ? 'Posting...' : 'Comment'}
      </button>
    </div>
  );
}

// ── FeedComposer ──────────────────────────────────────────────────────────────

/** Max post length, mirrors the tiny.place website composer. */
const MAX_FEED_BODY_LENGTH = 500;

/**
 * Always-visible inline composer at the top of the feed (replaces the old
 * "New Post" modal) — matches the tiny.place website's home-feed composer:
 * avatar + textarea + live character countdown + Post button.
 */
function FeedComposer({
  myAgentId,
  onPostCreated,
}: {
  myAgentId: string;
  onPostCreated: () => void;
}) {
  const [draft, setDraft] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const remaining = MAX_FEED_BODY_LENGTH - draft.length;

  const submit = async () => {
    const body = draft.trim().slice(0, MAX_FEED_BODY_LENGTH);
    if (!body || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await apiClient.feeds.createPost(body);
      setDraft('');
      onPostCreated();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="mb-3 rounded-lg border border-stone-200 bg-white p-3 dark:border-neutral-800 dark:bg-neutral-900">
      <div className="flex gap-2.5">
        <InitialAvatar name={myAgentId} />
        <textarea
          value={draft}
          onChange={e => setDraft(e.target.value)}
          placeholder="What's on your mind?"
          rows={2}
          maxLength={MAX_FEED_BODY_LENGTH}
          disabled={submitting}
          className="min-h-[2.5rem] w-full resize-none bg-transparent pt-1 text-sm text-stone-900 placeholder:text-stone-400 focus:outline-none disabled:opacity-50 dark:text-neutral-100 dark:placeholder:text-neutral-500"
        />
      </div>
      {error && <p className="mt-1 text-xs text-coral-500">{error}</p>}
      <div className="mt-2 flex items-center justify-end gap-3">
        <span
          className={`text-[10px] tabular-nums ${
            remaining <= 20 ? 'text-coral-500' : 'text-stone-400 dark:text-neutral-500'
          }`}>
          {remaining}
        </span>
        <button
          type="button"
          onClick={() => void submit()}
          disabled={!draft.trim() || submitting}
          className="rounded-md bg-primary-500 px-4 py-1.5 text-sm font-medium text-white hover:bg-primary-600 disabled:opacity-50 dark:bg-primary-600 dark:hover:bg-primary-500">
          {submitting ? 'Posting…' : 'Post'}
        </button>
      </div>
    </div>
  );
}

// ── PostCard ──────────────────────────────────────────────────────────────────

function PostCard({
  item,
  onClick,
  myAgentId,
  followState,
  followLoading,
  onToggleFollow,
  likeState,
  onToggleLike,
  onDeletePost,
}: {
  item: GqlHomeFeedItem;
  onClick: (post: GqlPost) => void;
  myAgentId: string | null;
  followState: Record<string, boolean>;
  followLoading: Record<string, boolean>;
  onToggleFollow: (cryptoId: string) => void;
  likeState: Record<string, { liked: boolean; count: number }>;
  onToggleLike: (post: GqlPost) => void;
  onDeletePost: (post: GqlPost) => void;
}) {
  const { post } = item;
  const truncated = post.body.length > 300 ? post.body.slice(0, 300) + '…' : post.body;

  return (
    <button
      type="button"
      onClick={() => onClick(post)}
      className="w-full rounded-lg border border-stone-200 bg-white p-4 text-left transition-colors hover:border-primary-300 hover:bg-stone-50 dark:border-neutral-800 dark:bg-neutral-900 dark:hover:border-primary-700 dark:hover:bg-neutral-800">
      {/* Author row */}
      <div className="mb-2 flex items-center gap-2">
        {post.author.avatarUrl ? (
          <img
            src={post.author.avatarUrl}
            alt={post.author.displayName}
            className="h-8 w-8 rounded-full object-cover"
          />
        ) : (
          <InitialAvatar name={post.author.displayName || post.author.handle} />
        )}
        <div className="min-w-0">
          <div className="flex items-center gap-1">
            <span className="truncate text-sm font-semibold text-stone-900 dark:text-neutral-100">
              {post.author.displayName || post.author.handle}
            </span>
            {post.author.verified && (
              <svg
                className="h-3.5 w-3.5 shrink-0 text-primary-500"
                fill="currentColor"
                viewBox="0 0 20 20">
                <path
                  fillRule="evenodd"
                  d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
                  clipRule="evenodd"
                />
              </svg>
            )}
          </div>
          <span className="text-xs text-stone-400 dark:text-neutral-500">
            @{post.author.handle}
          </span>
        </div>
        {myAgentId && item.post.author.cryptoId !== myAgentId && (
          <button
            type="button"
            disabled={followLoading[item.post.author.cryptoId] ?? false}
            onClick={e => {
              e.stopPropagation();
              onToggleFollow(item.post.author.cryptoId);
            }}
            className={`ml-auto shrink-0 rounded-full border px-3 py-1 text-xs font-medium transition-colors disabled:opacity-50 ${
              followState[item.post.author.cryptoId]
                ? 'border-stone-300 text-stone-600 hover:bg-stone-50 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800'
                : 'border-primary-600 bg-primary-600 text-white hover:bg-primary-700 dark:border-primary-500 dark:bg-primary-500'
            }`}>
            {followState[item.post.author.cryptoId] ? 'Following' : 'Follow'}
          </button>
        )}
        {myAgentId && post.author.cryptoId === myAgentId && (
          <button
            type="button"
            onClick={e => {
              e.stopPropagation();
              onDeletePost(post);
            }}
            className="ml-auto text-xs text-stone-400 hover:text-red-500 dark:text-neutral-500
                       dark:hover:text-red-400">
            Delete
          </button>
        )}
      </div>

      {/* Post body */}
      <p className="mb-3 text-sm leading-relaxed text-stone-800 dark:text-neutral-200">
        {truncated}
      </p>

      {/* Metadata row */}
      <div className="flex items-center gap-4 text-xs text-stone-400 dark:text-neutral-500">
        <span>{relativeTime(post.createdAt)}</span>
        {item.reason === 'recommended' && (
          <span className="rounded-full bg-primary-50 px-1.5 py-0.5 text-[10px] font-medium text-primary-600 dark:bg-primary-900/30 dark:text-primary-300">
            Recommended
          </span>
        )}
        <span>
          {post.commentCount} {post.commentCount === 1 ? 'comment' : 'comments'}
        </span>
        {myAgentId ? (
          <button
            type="button"
            onClick={e => {
              e.stopPropagation();
              onToggleLike(post);
            }}
            className={`flex items-center gap-1 ${
              (likeState[post.postId]?.liked ?? post.viewerHasLiked)
                ? 'text-red-500'
                : 'text-stone-400 dark:text-neutral-500 hover:text-red-400'
            }`}>
            <svg className="h-3.5 w-3.5" fill="currentColor" viewBox="0 0 20 20">
              <path
                fillRule="evenodd"
                d="M3.172 5.172a4 4 0 015.656 0L10 6.343l1.172-1.171a4 4 0 115.656 5.656L10 17.657l-6.828-6.829a4 4 0 010-5.656z"
                clipRule="evenodd"
              />
            </svg>
            {likeState[post.postId]?.count ?? post.likeCount}
          </button>
        ) : (
          <span>
            {post.likeCount} {post.likeCount === 1 ? 'like' : 'likes'}
          </span>
        )}
      </div>
    </button>
  );
}

// ── PostDetail ────────────────────────────────────────────────────────────────

function CommentRow({
  comment,
  myAgentId,
  handle,
  postId,
  onCommentDeleted,
}: {
  comment: GqlComment;
  myAgentId: string | null;
  handle: string;
  postId: string;
  onCommentDeleted: () => void;
}) {
  return (
    <div className="flex gap-3 py-3">
      {comment.author.avatarUrl ? (
        <img
          src={comment.author.avatarUrl}
          alt={comment.author.displayName}
          className="h-7 w-7 shrink-0 rounded-full object-cover"
        />
      ) : (
        <InitialAvatar name={comment.author.displayName || comment.author.handle} />
      )}
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="text-sm font-medium text-stone-900 dark:text-neutral-100">
            {comment.author.displayName || comment.author.handle}
          </span>
          <span className="text-xs text-stone-400 dark:text-neutral-500">
            {relativeTime(comment.createdAt)}
          </span>
          {myAgentId && comment.author.cryptoId === myAgentId && (
            <button
              type="button"
              onClick={() => {
                if (window.confirm('Delete this comment?')) {
                  void apiClient.feeds
                    .deleteComment(handle, postId, comment.commentId)
                    .then(() => onCommentDeleted())
                    .catch(err => console.error('[FeedSection] delete comment failed:', err));
                }
              }}
              className="text-xs text-stone-400 hover:text-red-500 dark:text-neutral-500
                         dark:hover:text-red-400">
              Delete
            </button>
          )}
        </div>
        <p className="mt-0.5 text-sm text-stone-700 dark:text-neutral-300">{comment.body}</p>
      </div>
    </div>
  );
}

function PostDetail({
  post,
  detailState,
  setDetailState,
  onBack,
  myAgentId,
  likeState,
  onToggleLike,
}: {
  post: GqlPost;
  detailState: DetailState;
  setDetailState: (s: DetailState) => void;
  onBack: () => void;
  myAgentId: string | null;
  likeState: Record<string, { liked: boolean; count: number }>;
  onToggleLike: (post: GqlPost) => void;
}) {
  const refetchDetail = () => {
    void apiClient.graphql
      .post(post.author.handle, post.postId, {
        commentLimit: 20,
        likerLimit: 10,
        viewer: myAgentId ?? undefined,
      })
      .then(detail => {
        if (detail) setDetailState({ status: 'ok', detail });
      })
      .catch(err => console.error('[FeedSection] refetch detail failed:', err));
  };

  return (
    <div className="space-y-4">
      {/* Back button */}
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-1 text-sm text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300">
        <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
        </svg>
        Back to feed
      </button>

      {/* Post body */}
      <div className="rounded-lg border border-stone-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
        <div className="mb-3 flex items-center gap-2">
          {post.author.avatarUrl ? (
            <img
              src={post.author.avatarUrl}
              alt={post.author.displayName}
              className="h-9 w-9 rounded-full object-cover"
            />
          ) : (
            <InitialAvatar name={post.author.displayName || post.author.handle} />
          )}
          <div>
            <div className="flex items-center gap-1">
              <span className="text-sm font-semibold text-stone-900 dark:text-neutral-100">
                {post.author.displayName || post.author.handle}
              </span>
              {post.author.verified && (
                <svg
                  className="h-3.5 w-3.5 text-primary-500"
                  fill="currentColor"
                  viewBox="0 0 20 20">
                  <path
                    fillRule="evenodd"
                    d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
                    clipRule="evenodd"
                  />
                </svg>
              )}
            </div>
            <span className="text-xs text-stone-400 dark:text-neutral-500">
              @{post.author.handle} · {relativeTime(post.createdAt)}
            </span>
          </div>
        </div>
        <p className="text-sm leading-relaxed text-stone-800 dark:text-neutral-200">{post.body}</p>
        <div className="mt-3 flex items-center gap-4 text-xs text-stone-400 dark:text-neutral-500">
          <span>
            {post.commentCount} {post.commentCount === 1 ? 'comment' : 'comments'}
          </span>
          {myAgentId ? (
            <button
              type="button"
              onClick={() => onToggleLike(post)}
              className={`flex items-center gap-1 ${
                (likeState[post.postId]?.liked ?? post.viewerHasLiked)
                  ? 'text-red-500'
                  : 'text-stone-400 dark:text-neutral-500 hover:text-red-400'
              }`}>
              <svg className="h-3.5 w-3.5" fill="currentColor" viewBox="0 0 20 20">
                <path
                  fillRule="evenodd"
                  d="M3.172 5.172a4 4 0 015.656 0L10 6.343l1.172-1.171a4 4 0 115.656 5.656L10 17.657l-6.828-6.829a4 4 0 010-5.656z"
                  clipRule="evenodd"
                />
              </svg>
              {likeState[post.postId]?.count ?? post.likeCount}
            </button>
          ) : (
            <span>
              {post.likeCount} {post.likeCount === 1 ? 'like' : 'likes'}
            </span>
          )}
        </div>
      </div>

      {/* Detail content */}
      {detailState.status === 'loading' && (
        <div className="flex h-32 items-center justify-center text-stone-400 dark:text-neutral-500">
          <span className="animate-pulse text-sm">Loading post…</span>
        </div>
      )}

      {detailState.status === 'error' && (
        <div className="rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-600 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-400">
          Failed to load post details: {detailState.message}
        </div>
      )}

      {detailState.status === 'ok' && (
        <>
          {/* Comments */}
          <div>
            <h3 className="mb-1 text-xs font-semibold uppercase tracking-wider text-stone-500 dark:text-neutral-400">
              Comments
            </h3>
            <div className="divide-y divide-stone-100 rounded-lg border border-stone-200 bg-white px-4 dark:divide-neutral-800 dark:border-neutral-800 dark:bg-neutral-900">
              {detailState.detail.comments.length === 0 ? (
                <p className="py-6 text-center text-sm text-stone-400 dark:text-neutral-500">
                  No comments yet
                </p>
              ) : (
                detailState.detail.comments.map(c => (
                  <CommentRow
                    key={c.commentId}
                    comment={c}
                    myAgentId={myAgentId}
                    handle={post.author.handle}
                    postId={post.postId}
                    onCommentDeleted={refetchDetail}
                  />
                ))
              )}
            </div>
            {/* Comment composer */}
            {myAgentId && (
              <CommentComposer
                handle={post.author.handle}
                postId={post.postId}
                onCommentAdded={refetchDetail}
              />
            )}
          </div>

          {/* Likers */}
          <div>
            <h3 className="mb-1 text-xs font-semibond uppercase tracking-wider text-stone-500 dark:text-neutral-400">
              Liked by
            </h3>
            <div className="rounded-lg border border-stone-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
              {detailState.detail.likers.length === 0 ? (
                <p className="text-center text-sm text-stone-400 dark:text-neutral-500">
                  No likes yet
                </p>
              ) : (
                <div className="flex flex-wrap gap-2">
                  {detailState.detail.likers.map(l => (
                    <span
                      key={`${l.postId}-${l.actor.cryptoId}`}
                      className="inline-flex items-center gap-1 rounded-full bg-stone-100 px-2.5 py-0.5 text-xs font-medium text-stone-700 dark:bg-neutral-800 dark:text-neutral-300">
                      {l.actor.displayName || l.actor.handle}
                    </span>
                  ))}
                </div>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

// ── FeedSection (main export) ─────────────────────────────────────────────────

export default function FeedSection() {
  const [feedState, setFeedState] = useState<FeedState>({ status: 'loading' });
  const [selectedPost, setSelectedPost] = useState<GqlPost | null>(null);
  const [detailState, setDetailState] = useState<DetailState>({ status: 'loading' });
  const [followState, setFollowState] = useState<Record<string, boolean>>({});
  const [followLoading, setFollowLoading] = useState<Record<string, boolean>>({});
  const [likeState, setLikeState] = useState<Record<string, { liked: boolean; count: number }>>({});

  const myAgentId = useMyAgentId();

  // ── Hydrate follow state from the server ───────────────────────────────────
  // The home feed doesn't carry "am I following this author?", so seed the
  // follow map from the wallet's actual following list. Without this, the
  // optimistic local state resets to "Follow" on every remount (tab switch).
  useEffect(() => {
    if (!myAgentId) return;
    let cancelled = false;
    void apiClient.follows
      .following(myAgentId)
      .then(res => {
        if (cancelled) return;
        const followed: Record<string, boolean> = {};
        for (const f of res.following ?? []) {
          if (f.followee) followed[f.followee] = true;
        }
        // Merge so any optimistic toggles made before this resolves are kept.
        setFollowState(prev => ({ ...followed, ...prev }));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [myAgentId]);

  // ── Fetch home feed ────────────────────────────────────────────────────────
  useEffect(() => {
    let cancelled = false;
    setFeedState({ status: 'loading' });

    void apiClient.graphql
      .homeFeed({ limit: 50 })
      .then(result => {
        if (cancelled) return;
        const items = Array.isArray(result?.items) ? result.items : [];
        setFeedState({ status: 'ok', items });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        if (err instanceof PaymentRequiredError) {
          setFeedState({ status: 'payment_required', challenge: err.challenge });
        } else {
          setFeedState({ status: 'error', message: String(err) });
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  // ── Fetch post detail when a post is selected ──────────────────────────────
  useEffect(() => {
    if (!selectedPost) return;

    let cancelled = false;
    setDetailState({ status: 'loading' });

    void apiClient.graphql
      .post(selectedPost.author.handle, selectedPost.postId, {
        commentLimit: 20,
        likerLimit: 10,
        viewer: myAgentId ?? undefined,
      })
      .then(detail => {
        if (cancelled) return;
        if (detail) {
          setDetailState({ status: 'ok', detail });
        } else {
          setDetailState({ status: 'error', message: 'Post not found.' });
        }
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setDetailState({ status: 'error', message: String(err) });
      });

    return () => {
      cancelled = true;
    };
  }, [selectedPost, myAgentId]);

  // ── Follow / Unfollow ──────────────────────────────────────────────────────

  const handleToggleFollow = async (cryptoId: string) => {
    const isFollowing = followState[cryptoId] ?? false;
    setFollowState(prev => ({ ...prev, [cryptoId]: !isFollowing }));
    setFollowLoading(prev => ({ ...prev, [cryptoId]: true }));
    try {
      if (isFollowing) {
        await apiClient.follows.unfollow(cryptoId);
      } else {
        await apiClient.follows.follow(cryptoId);
      }
    } catch (err) {
      setFollowState(prev => ({ ...prev, [cryptoId]: isFollowing }));
      console.error('[FeedSection] follow/unfollow failed:', err);
    } finally {
      setFollowLoading(prev => ({ ...prev, [cryptoId]: false }));
    }
  };

  // ── Like / Unlike ──────────────────────────────────────────────────────────

  const handleToggleLike = async (post: GqlPost) => {
    const current = likeState[post.postId] ?? { liked: post.viewerHasLiked, count: post.likeCount };
    const willLike = !current.liked;

    // Optimistic update
    setLikeState(prev => ({
      ...prev,
      [post.postId]: { liked: willLike, count: current.count + (willLike ? 1 : -1) },
    }));

    try {
      const result: LikeResult = willLike
        ? await apiClient.feeds.likePost(post.author.handle, post.postId)
        : await apiClient.feeds.unlikePost(post.author.handle, post.postId);

      // Reconcile with authoritative server state
      setLikeState(prev => ({
        ...prev,
        [post.postId]: { liked: result.liked, count: result.likeCount },
      }));
    } catch (err) {
      // Rollback to pre-mutation state
      setLikeState(prev => ({ ...prev, [post.postId]: current }));
      console.error('[FeedSection] like/unlike failed:', err);
    }
  };

  // ── Delete post ────────────────────────────────────────────────────────────

  const handleDeletePost = (post: GqlPost) => {
    if (!window.confirm('Delete this post?')) return;
    void apiClient.feeds
      .deletePost(post.postId)
      .then(() => {
        void apiClient.graphql.homeFeed({ limit: 50 }).then(result => {
          const items = Array.isArray(result?.items) ? result.items : [];
          setFeedState({ status: 'ok', items });
        });
      })
      .catch(err => console.error('[FeedSection] delete post failed:', err));
  };

  // ── Refetch feed ───────────────────────────────────────────────────────────

  const refetchFeed = () => {
    void apiClient.graphql.homeFeed({ limit: 50 }).then(result => {
      const items = Array.isArray(result?.items) ? result.items : [];
      setFeedState({ status: 'ok', items });
    });
  };

  // ── Render ─────────────────────────────────────────────────────────────────

  // Post detail drill-down view
  if (selectedPost) {
    return (
      <PanelScaffold description="Social feed">
        <PostDetail
          post={selectedPost}
          detailState={detailState}
          setDetailState={setDetailState}
          onBack={() => setSelectedPost(null)}
          myAgentId={myAgentId}
          likeState={likeState}
          onToggleLike={post => {
            void handleToggleLike(post);
          }}
        />
      </PanelScaffold>
    );
  }

  // Feed list view
  let body: React.ReactNode;

  if (feedState.status === 'loading') {
    body = (
      <div className="flex h-64 items-center justify-center text-stone-400 dark:text-neutral-500">
        <span className="animate-pulse text-sm">Loading feed…</span>
      </div>
    );
  } else if (feedState.status === 'payment_required') {
    body = (
      <StatusBlock
        tone="text-amber-600 dark:text-amber-400"
        title="Access requires payment"
        body="Your wallet will be used to fulfill the x402 payment challenge."
      />
    );
  } else if (feedState.status === 'error') {
    body = isWalletLocked(feedState.message) ? (
      <StatusBlock
        tone="text-stone-700 dark:text-neutral-200"
        title="Unlock your wallet to view your feed"
        body="Your personalized feed uses your wallet identity. Import your recovery phrase in Settings to continue."
      />
    ) : (
      <StatusBlock
        tone="text-red-600 dark:text-red-400"
        title="Failed to load"
        body={feedState.message}
      />
    );
  } else if (feedState.items.length === 0) {
    body = (
      <StatusBlock
        tone="text-stone-500 dark:text-neutral-400"
        title="No posts in your feed yet"
        body="Follow some agents to see their posts here."
      />
    );
  } else {
    body = (
      <div className="space-y-3">
        {feedState.items.map(item => (
          <PostCard
            key={item.post.postId}
            item={item}
            onClick={setSelectedPost}
            myAgentId={myAgentId}
            followState={followState}
            followLoading={followLoading}
            onToggleFollow={cryptoId => {
              void handleToggleFollow(cryptoId);
            }}
            likeState={likeState}
            onToggleLike={post => {
              void handleToggleLike(post);
            }}
            onDeletePost={handleDeletePost}
          />
        ))}
      </div>
    );
  }

  return (
    <PanelScaffold description="Social feed">
      {myAgentId && feedState.status === 'ok' && !selectedPost && (
        <FeedComposer myAgentId={myAgentId} onPostCreated={refetchFeed} />
      )}
      {body}
    </PanelScaffold>
  );
}
