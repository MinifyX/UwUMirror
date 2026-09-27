import type { ReactNode } from 'react';
import { NYU, NyuFigure, Paw, Sticker } from '@nyu/Nyu';
import { Heart, NyuScene, Phone, Shadow, Star, Waves } from '@nyu/scenes';

// Installer scenes on the same 320 × 220 canvas as the app's scenes.

function Canvas({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="-10 -10 340 230"
      className="nyu-host nyu-blink w-full overflow-visible"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  );
}

/**
 * Nyu hops and tosses hearts and little phones into a moving box; on the box a
 * phone sends out its waves, the way the real ones will once she is done.
 */
export function WorkingScene() {
  const S = { stroke: NYU.outline, strokeWidth: 6 } as const;
  const tossed: ReactNode[] = [
    <Heart key="heart" x={118} y={92} size={0.9} />,
    <Phone key="phone" x={118} y={92} rotate={-12} size={0.55} />,
    <Heart key="mint" x={118} y={92} size={0.8} fill={NYU.mint} />,
  ];
  return (
    <Canvas>
      <Shadow cx={150} rx={120} />
      <Sticker edge={18}>
        <path d="M206 134 L178 116 L190 102 L226 134Z" fill={NYU.kraftLight} {...S} />
        <path d="M290 134 L318 116 L306 102 L270 134Z" fill={NYU.kraftLight} {...S} />
        <path d="M200 134 H296 L288 200 H208Z" fill={NYU.kraft} {...S} />
        <Phone x={248} y={172} rotate={90} size={0.52} />
      </Sticker>
      <Waves x={248} y={150} rotate={-90} />
      {tossed.map((item, index) => (
        <g key={index} className="setup-toss" style={{ animationDelay: `${index * 0.55}s` }}>
          <Sticker edge={12}>{item}</Sticker>
        </g>
      ))}
      <g className="setup-hop">
        <NyuFigure
          mood="happy"
          x={92}
          y={140}
          scale={0.56}
          tilt={-4}
          edge={30}
          front={<Paw x={232} y={110} />}
        />
      </g>
      <Sticker edge={12}>
        <Star x={292} y={44} r={11} />
        <Star x={30} y={40} r={8} />
      </Sticker>
    </Canvas>
  );
}

export function GoodbyeScene() {
  return <NyuScene name="goodbye" className="w-full" />;
}

export function WelcomeScene() {
  return <NyuScene name="welcome" className="w-full" />;
}

export function DoneScene() {
  return (
    <div className="setup-pop w-full">
      <NyuScene name="done" className="w-full" />
    </div>
  );
}

export function ErrorScene() {
  return <NyuScene name="loadError" className="w-full" />;
}

export function PuzzledScene() {
  return <NyuScene name="puzzled" className="w-full" />;
}
