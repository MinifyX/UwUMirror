import type { ReactNode } from 'react';
import { NYU, NyuFigure, Paw, Sticker } from './Nyu';

// Every scene is drawn on a 320 × 220 canvas, the same as UwUMail's. Nyu sits
// at about 0.6 scale, so props use a 6 px outline and an 18 px edge to match.

const S = { stroke: NYU.outline, strokeWidth: 6 } as const;
const EDGE = 18;
/** Nyu's own edge at scene scale: 30 × 0.6 ≈ the props' 18 px. */
const NYU_EDGE = 30;

export function Shadow({ cx = 160, rx = 104 }: { cx?: number; rx?: number }) {
  return (
    <ellipse
      className="no-edge"
      cx={cx}
      cy="204"
      rx={rx}
      ry="8"
      fill={NYU.outline}
      opacity="0.08"
    />
  );
}

export function Star({
  x,
  y,
  r = 12,
  className,
}: {
  x: number;
  y: number;
  r?: number;
  className?: string;
}) {
  const k = r * 0.2;
  return (
    <path
      className={className}
      d={`M${x} ${y - r} Q${x + k} ${y - k} ${x + r} ${y} Q${x + k} ${y + k} ${x} ${y + r} Q${x - k} ${y + k} ${x - r} ${y} Q${x - k} ${y - k} ${x} ${y - r}Z`}
      fill={NYU.star}
      stroke={NYU.outline}
      strokeWidth={r > 10 ? 4 : 3}
    />
  );
}

export function Heart({
  x,
  y,
  size = 1,
  fill = NYU.body,
}: {
  x: number;
  y: number;
  size?: number;
  fill?: string;
}) {
  return (
    <path
      transform={`translate(${x} ${y}) scale(${size})`}
      d="M0 13 C-15 3 -18 -4 -17 -8 C-16 -15 -7 -16 -3 -11 L0 -8 L3 -11 C7 -16 16 -15 17 -8 C18 -4 15 3 0 13Z"
      fill={fill}
      stroke={NYU.outline}
      strokeWidth={4 / size}
    />
  );
}

/**
 * A phone, the kind that mirrors itself onto Nyu: rounded body, a notch, and
 * on its screen a tiny heart — the picture that is about to travel.
 */
export function Phone({
  x,
  y,
  rotate = 0,
  size = 1,
  screen = 'heart',
}: {
  x: number;
  y: number;
  rotate?: number;
  size?: number;
  /** What the phone shows: a heart, or the squares of a pairing code. */
  screen?: 'heart' | 'code';
}) {
  return (
    <g transform={`translate(${x} ${y}) rotate(${rotate}) scale(${size})`}>
      <rect x="-20" y="-34" width="40" height="68" rx="9" fill={NYU.lilac} {...S} strokeWidth={5} />
      <rect className="no-edge" x="-13" y="-25" width="26" height="46" rx="4" fill={NYU.night} />
      <rect className="no-edge" x="-6" y="-31" width="12" height="3" rx="1.5" fill={NYU.outline} />
      {screen === 'heart' ? (
        <g className="no-edge">
          <Heart x={0} y={-2} size={0.45} fill={NYU.body} />
        </g>
      ) : (
        <g className="no-edge" fill={NYU.paper}>
          <rect x="-10" y="-18" width="8" height="8" rx="1" />
          <rect x="2" y="-18" width="8" height="8" rx="1" />
          <rect x="-10" y="-6" width="8" height="8" rx="1" />
          <rect x="4" y="-4" width="4" height="4" />
          <rect x="-2" y="4" width="4" height="4" />
          <rect x="4" y="8" width="5" height="5" />
          <rect x="-9" y="8" width="5" height="5" />
        </g>
      )}
    </g>
  );
}

/** Three arcs from a phone towards Nyu; with `live` they light up in turn. */
export function Waves({
  x,
  y,
  rotate = 0,
  live = true,
}: {
  x: number;
  y: number;
  rotate?: number;
  live?: boolean;
}) {
  return (
    <g
      className={live ? 'nyu-waves' : undefined}
      transform={`translate(${x} ${y}) rotate(${rotate})`}
      fill="none"
      stroke={NYU.violet}
      strokeWidth={5}
    >
      <path d="M0 -8 q7 8 0 16" />
      <path d="M10 -16 q13 16 0 32" />
      <path d="M20 -24 q19 24 0 48" />
    </g>
  );
}

/** First start: Nyu says hello. */
function Welcome() {
  return (
    <>
      <Shadow />
      <path
        d="M246 44 q12 9 10 25 M262 32 q16 13 14 35"
        fill="none"
        stroke={NYU.outline}
        strokeWidth={4}
        opacity="0.4"
      />
      <NyuFigure
        mood="happy"
        x={150}
        y={134}
        scale={0.62}
        tilt={-6}
        edge={NYU_EDGE}
        front={<Paw x={238} y={104} className="nyu-wave" />}
      />
      <Sticker edge={12}>
        <Heart x={50} y={62} size={0.95} />
        <Star x={286} y={150} r={11} />
        <Star x={36} y={150} r={8} />
      </Sticker>
    </>
  );
}

/** Ready: a phone off to the side, its picture on its way to Nyu. */
function Waiting() {
  return (
    <>
      <Shadow cx={180} />
      <Sticker edge={EDGE}>
        <Phone x={48} y={112} rotate={-10} />
      </Sticker>
      <Waves x={84} y={106} />
      <NyuFigure mood="happy" x={196} y={130} scale={0.6} tilt={4} edge={NYU_EDGE} />
      <Sticker edge={12}>
        <Star x={296} y={46} r={10} className="nyu-twinkle" />
        <Heart x={46} y={40} size={0.7} fill={NYU.mint} />
      </Sticker>
    </>
  );
}

/** A picture arriving: the phone hops, the waves run, Nyu is all sparkles. */
function Connecting() {
  return (
    <>
      <Shadow cx={170} />
      <g className="nyu-hop">
        <Sticker edge={EDGE}>
          <Phone x={52} y={108} rotate={-8} />
        </Sticker>
      </g>
      <Waves x={88} y={104} />
      <NyuFigure mood="sparkle" x={200} y={128} scale={0.58} tilt={-3} edge={NYU_EDGE} />
      <Sticker edge={10}>
        <g className="nyu-sparks">
          <Star x={300} y={60} r={8} />
          <Star x={296} y={160} r={6} />
        </g>
      </Sticker>
    </>
  );
}

/** Pairing: Nyu holds up a phone with a code on it. */
function Pair() {
  return (
    <>
      <Shadow cx={150} />
      <NyuFigure
        mood="happy"
        x={130}
        y={132}
        scale={0.58}
        tilt={-4}
        edge={NYU_EDGE}
        front={<Paw x={228} y={128} />}
      />
      <Sticker edge={EDGE}>
        <g className="nyu-bob">
          <Phone x={258} y={92} rotate={12} size={1.2} screen="code" />
        </g>
      </Sticker>
      <Sticker edge={12}>
        <Star x={40} y={56} r={9} className="nyu-twinkle" />
        <Heart x={300} y={180} size={0.6} />
      </Sticker>
    </>
  );
}

const CONFETTI: [x: number, y: number, rotate: number, fill: string][] = [
  [42, 40, -20, NYU.body],
  [78, 16, 30, NYU.star],
  [118, 30, 70, NYU.mint],
  [210, 22, -40, NYU.lilac],
  [250, 44, 15, NYU.body],
  [284, 20, 60, NYU.sky],
  [30, 108, 45, NYU.sky],
  [292, 104, -30, NYU.star],
  [48, 170, 20, NYU.lilac],
  [276, 172, -60, NYU.mint],
];

/** Done: Nyu cheers with both paws up. */
function Done() {
  return (
    <>
      <Shadow />
      <Sticker edge={10}>
        {CONFETTI.map(([x, y, rotate, fill]) => (
          <rect
            key={`${x}-${y}`}
            x={x - 7}
            y={y - 4}
            width="14"
            height="8"
            rx="2"
            transform={`rotate(${rotate} ${x} ${y})`}
            fill={fill}
            stroke={NYU.outline}
            strokeWidth={3}
          />
        ))}
      </Sticker>
      <NyuFigure
        mood="cheer"
        x={160}
        y={138}
        scale={0.62}
        edge={NYU_EDGE}
        front={
          <>
            <Paw x={24} y={112} />
            <Paw x={232} y={112} />
          </>
        }
      />
      <Sticker edge={12}>
        <Star x={160} y={34} r={11} />
      </Sticker>
    </>
  );
}

/** Something failed: the waves broke off halfway, Nyu is sad. */
function LoadError() {
  return (
    <>
      <Shadow cx={170} />
      <Sticker edge={EDGE}>
        <Phone x={50} y={120} rotate={-18} />
      </Sticker>
      <g fill="none" stroke={NYU.violet} strokeWidth={5} opacity={0.5}>
        <path d="M86 104 q7 8 0 16" />
        <path d="M100 96 l6 8 M108 110 l4 6" />
      </g>
      <NyuFigure mood="sad" x={200} y={124} scale={0.6} tilt={6} edge={NYU_EDGE} />
    </>
  );
}

/** Something needs a decision first: Nyu holds up a page with a question mark. */
function Puzzled() {
  return (
    <>
      <Shadow cx={150} />
      <NyuFigure mood="puzzled" x={112} y={134} scale={0.6} tilt={-8} edge={NYU_EDGE} />
      <Sticker edge={EDGE}>
        <g transform="rotate(8 226 118)">
          <path d="M190 58 H240 L262 80 V176 H190Z" fill={NYU.paper} {...S} />
          <path d="M240 58 V80 H262" fill={NYU.screen} {...S} />
          <path
            d="M212 106 q0 -15 15 -15 q15 0 15 13 q0 10 -13 14 v8"
            fill="none"
            stroke={NYU.body}
            strokeWidth={9}
          />
          <circle cx="229" cy="145" r="5.5" fill={NYU.body} />
        </g>
        <ellipse cx="186" cy="138" rx="12" ry="10" fill={NYU.body} {...S} />
      </Sticker>
    </>
  );
}

/** Saying goodbye: Nyu waves with a little tear. */
function Goodbye() {
  return (
    <>
      <Shadow />
      <NyuFigure
        mood="sad"
        x={160}
        y={134}
        scale={0.62}
        tilt={4}
        edge={NYU_EDGE}
        front={<Paw x={240} y={104} className="nyu-wave" />}
      />
      <Sticker edge={12}>
        <Heart x={58} y={58} size={0.85} fill={NYU.lilac} />
        <Star x={280} y={40} r={9} />
      </Sticker>
    </>
  );
}

/** Nothing going on: Nyu naps, a little "z" floating up. */
function Sleepy() {
  return (
    <>
      <Shadow cx={160} rx={90} />
      <NyuFigure mood="sleepy" x={160} y={140} scale={0.6} tilt={8} edge={NYU_EDGE} />
      <g className="nyu-zzz" fill="none" stroke={NYU.violet} strokeWidth={5}>
        <path d="M232 70 h16 l-16 16 h16" />
        <path d="M258 40 h11 l-11 11 h11" />
      </g>
    </>
  );
}

const SCENES = {
  welcome: Welcome,
  waiting: Waiting,
  connecting: Connecting,
  pair: Pair,
  done: Done,
  loadError: LoadError,
  puzzled: Puzzled,
  goodbye: Goodbye,
  sleepy: Sleepy,
} satisfies Record<string, () => ReactNode>;

export type SceneName = keyof typeof SCENES;

/** A small illustration of Nyu for empty and error states. Decorative only. */
export function NyuScene({ name, className }: { name: SceneName; className?: string }) {
  const Scene = SCENES[name];
  return (
    <svg
      viewBox="-10 -10 340 230"
      className={className ? `nyu-host nyu-blink ${className}` : 'nyu-host nyu-blink'}
      style={{ overflow: 'visible' }}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <Scene />
    </svg>
  );
}
