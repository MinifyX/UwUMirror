/**
 * The streams the page knows about, each with its player, as a small store.
 *
 * Start and end come as events, video on a channel of its own — the two can
 * overtake each other, so a player is made by whichever arrives first.
 */

import { useSyncExternalStore } from 'react';
import { api, onStreamMessage, subscribeVideo, type StreamMessage, type StreamState } from './api';
import { StreamPlayer } from './player';

export type Stream = StreamState & { player: StreamPlayer };

export type EndedStream = Extract<StreamMessage, { type: 'ended' }>;

let streams: Stream[] = [];
const players = new Map<number, StreamPlayer>();
const listeners = new Set<() => void>();
const endListeners = new Set<(ended: EndedStream) => void>();
const startListeners = new Set<(stream: Stream) => void>();
/** Streams that ended, so late video for them doesn't bring back a player. */
const ended = new Set<number>();

function publish(next: Stream[]) {
  streams = next;
  for (const listener of listeners) listener();
}

function playerFor(id: number): StreamPlayer {
  let player = players.get(id);
  if (!player) {
    player = new StreamPlayer();
    players.set(id, player);
  }
  return player;
}

function upsert(state: StreamState): Stream {
  const player = playerFor(state.id);
  if (state.width > 0 && state.height > 0) player.setSize(state.width, state.height);
  const stream = { ...state, player };
  const index = streams.findIndex((s) => s.id === state.id);
  publish(
    index === -1 ? [...streams, stream] : streams.map((s) => (s.id === state.id ? stream : s)),
  );
  return stream;
}

function handle(message: StreamMessage) {
  switch (message.type) {
    case 'started': {
      const known = streams.some((s) => s.id === message.stream.id);
      const stream = upsert(message.stream);
      if (!known) for (const listener of startListeners) listener(stream);
      break;
    }
    case 'updated':
      if (streams.some((s) => s.id === message.stream.id)) upsert(message.stream);
      break;
    case 'ended': {
      ended.add(message.id);
      players.get(message.id)?.close();
      players.delete(message.id);
      publish(streams.filter((s) => s.id !== message.id));
      for (const listener of endListeners) listener(message);
      break;
    }
  }
}

let started = false;

/** Listens for streams and video; call once when the app starts. */
export async function startStreams() {
  if (started) return;
  started = true;
  await onStreamMessage(handle);
  for (const state of await api.streams()) upsert(state);
  await subscribeVideo((packet) => {
    // Video for a stream that already ended is dropped; for one whose start
    // is still on its way, the player is made now.
    if (ended.has(packet.id)) return;
    playerFor(packet.id).push(packet.key, packet.pts, packet.data);
  });
}

export function onStreamStarted(listener: (stream: Stream) => void): () => void {
  startListeners.add(listener);
  return () => startListeners.delete(listener);
}

export function onStreamEnded(listener: (ended: EndedStream) => void): () => void {
  endListeners.add(listener);
  return () => endListeners.delete(listener);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useStreams(): Stream[] {
  return useSyncExternalStore(subscribe, () => streams);
}
