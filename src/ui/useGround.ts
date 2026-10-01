// Which ground the app is on, for the few places that cannot read a CSS token.
//
// Almost nothing needs this: every colour in this app comes from a token, and tokens already flip with
// `data-theme` on <html> without anyone being told. Monaco is the exception -- it paints its own canvas
// from a named theme, so it has to be handed the answer, and hardcoding `vs-dark` left the editor in
// dark syntax colours on a white page.
//
// The attribute is watched rather than read once, so a future theme switch needs no change here.
import { useEffect, useState } from 'react';

export type Ground = 'light' | 'dark';

function currentGround(): Ground {
  return document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
}

export function useGround(): Ground {
  const [ground, setGround] = useState<Ground>(currentGround);

  useEffect(() => {
    const observer = new MutationObserver(() => setGround(currentGround()));
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });
    return () => observer.disconnect();
  }, []);

  return ground;
}
